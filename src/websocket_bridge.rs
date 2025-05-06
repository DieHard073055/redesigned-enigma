// src/websocket_bridge.rs
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tonic::transport::Channel;

// Import generated protobuf code
use echo::echo_service_client::EchoServiceClient;
use echo::EchoRequest;

// Include the generated protobuf code
pub mod echo {
    tonic::include_proto!("echo");
}

// Structures for JSON communication with WebSocket clients
#[derive(Serialize, Deserialize, Debug)]
struct WebSocketRequest {
    method: String,
    message: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct WebSocketResponse {
    method: String,
    message: String,
    timestamp: i64,
}

async fn handle_websocket_connection(
    websocket: tokio_tungstenite::WebSocketStream<TcpStream>,
    client: EchoServiceClient<Channel>,
) {
    let (ws_sender, mut ws_receiver) = websocket.split();
    let mut client = client;

    // Create the main Arc<Mutex<...>> that will be cloned for each use
    let ws_sender_arc = Arc::new(Mutex::new(ws_sender));

    while let Some(msg) = ws_receiver.next().await {
        if let Ok(msg) = msg {
            if msg.is_text() || msg.is_binary() {
                let text = msg.to_text().unwrap_or_default();
                println!("Received WebSocket message: {}", text);

                // Parse the JSON request
                match serde_json::from_str::<WebSocketRequest>(text) {
                    Ok(request) => {
                        match request.method.as_str() {
                            "echo" => {
                                // Create a clone for this method handler
                                let ws_sender = Arc::clone(&ws_sender_arc);

                                // Handle unary call
                                let grpc_request = tonic::Request::new(EchoRequest {
                                    message: request.message.clone(), // Clone if needed
                                });

                                match client.echo(grpc_request).await {
                                    Ok(response) => {
                                        let response = response.into_inner();
                                        let ws_response = WebSocketResponse {
                                            method: "echo".to_string(),
                                            message: response.message,
                                            timestamp: response.timestamp,
                                        };
                                        let json = serde_json::to_string(&ws_response).unwrap();

                                        // Obtain lock and send the message
                                        let mut sender = ws_sender.lock().await;
                                        if let Err(e) = sender.send(Message::Text(json)).await {
                                            println!("Error sending WebSocket response: {:?}", e);
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        println!("gRPC error: {:?}", e);
                                        // Obtain lock and send the error message
                                        let mut sender = ws_sender.lock().await;
                                        if let Err(e) = sender
                                            .send(Message::Text(format!("Error: {:?}", e)))
                                            .await
                                        {
                                            println!(
                                                "Error sending WebSocket error response: {:?}",
                                                e
                                            );
                                            break;
                                        }
                                    }
                                }
                            }
                            "serverStream" => {
                                // Create a dedicated clone for the task
                                let ws_sender_for_task = Arc::clone(&ws_sender_arc);

                                let grpc_request = tonic::Request::new(EchoRequest {
                                    message: request.message.clone(),
                                });

                                match client.server_stream(grpc_request).await {
                                    Ok(response) => {
                                        let mut stream = response.into_inner();

                                        // Use ws_sender_for_task in the spawned task
                                        tokio::spawn(async move {
                                            while let Some(result) = stream.next().await {
                                                match result {
                                                    Ok(response) => {
                                                        let ws_response = WebSocketResponse {
                                                            method: "serverStream".to_string(),
                                                            message: response.message,
                                                            timestamp: response.timestamp,
                                                        };

                                                        // Obtain the lock and handle errors
                                                        let mut sender =
                                                            ws_sender_for_task.lock().await;
                                                        // Send the WebSocket response
                                                        if let Err(e) = sender
                                                            .send(Message::Text(
                                                                serde_json::to_string(&ws_response)
                                                                    .unwrap(),
                                                            ))
                                                            .await
                                                        {
                                                            eprintln!("Failed to send WebSocket message: {:?}", e);
                                                            break;
                                                        }
                                                    }
                                                    Err(e) => {
                                                        eprintln!("Error receiving gRPC server stream: {:?}", e);
                                                        break;
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    Err(e) => {
                                        // Use the main Arc for error handling
                                        let ws_sender = Arc::clone(&ws_sender_arc);
                                        eprintln!("Error calling server stream: {:?}", e);

                                        let mut sender = ws_sender.lock().await;
                                        if let Err(e) = sender
                                            .send(Message::Text(format!("Stream Error: {:?}", e)))
                                            .await
                                        {
                                            println!("Error sending stream error: {:?}", e);
                                        }
                                    }
                                }
                            }
                            "bidirectionalStream" => {
                                // Create clones for each use
                                let ws_sender_for_stream = Arc::clone(&ws_sender_arc);
                                let ws_sender_for_ready = Arc::clone(&ws_sender_arc);

                                // Start bidirectional streaming
                                let (tx, rx) = mpsc::channel(32);
                                let stream = ReceiverStream::new(rx);
                                let request = tonic::Request::new(stream);

                                match client.bidirectional_stream(request).await {
                                    Ok(response) => {
                                        let mut stream = response.into_inner();

                                        // Send the first message to start the stream
                                        tx.send(EchoRequest {
                                            message: "Initial message".to_string(),
                                        })
                                        .await
                                        .unwrap();

                                        // Handle incoming gRPC responses in a separate task
                                        tokio::spawn(async move {
                                            while let Some(result) = stream.next().await {
                                                match result {
                                                    Ok(response) => {
                                                        let ws_response = WebSocketResponse {
                                                            method: "bidirectionalStream"
                                                                .to_string(),
                                                            message: response.message,
                                                            timestamp: response.timestamp,
                                                        };
                                                        let json =
                                                            serde_json::to_string(&ws_response)
                                                                .unwrap();

                                                        let mut sender =
                                                            ws_sender_for_stream.lock().await;
                                                        if let Err(e) =
                                                            sender.send(Message::Text(json)).await
                                                        {
                                                            println!("Error sending WebSocket bidir response: {:?}", e);
                                                            break;
                                                        }
                                                    }
                                                    Err(e) => {
                                                        println!("gRPC bidir error: {:?}", e);
                                                        let mut sender =
                                                            ws_sender_for_stream.lock().await;
                                                        if let Err(e) = sender
                                                            .send(Message::Text(format!(
                                                                "Bidir error: {:?}",
                                                                e
                                                            )))
                                                            .await
                                                        {
                                                            println!("Error sending WebSocket error response: {:?}", e);
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                        });

                                        // Send a ready message using a different clone
                                        let ready_msg = serde_json::to_string(&WebSocketResponse {
                                            method: "bidirectionalStreamReady".to_string(),
                                            message:
                                                "Stream established, send messages to continue"
                                                    .to_string(),
                                            timestamp: 0,
                                        })
                                        .unwrap();

                                        let mut sender = ws_sender_for_ready.lock().await;
                                        if let Err(e) = sender.send(Message::Text(ready_msg)).await
                                        {
                                            println!(
                                                "Error sending WebSocket ready message: {:?}",
                                                e
                                            );
                                        }

                                        // Keep the tx alive in a separate task
                                        tokio::spawn(async move {
                                            // Just a placeholder to keep tx alive
                                            tokio::time::sleep(tokio::time::Duration::from_secs(
                                                3600,
                                            ))
                                            .await;
                                        });
                                    }
                                    Err(e) => {
                                        // Use a new clone for error handling
                                        let ws_sender = Arc::clone(&ws_sender_arc);
                                        println!(
                                            "gRPC error setting up bidirectional stream: {:?}",
                                            e
                                        );

                                        let mut sender = ws_sender.lock().await;
                                        if let Err(e) = sender
                                            .send(Message::Text(format!("Error: {:?}", e)))
                                            .await
                                        {
                                            println!(
                                                "Error sending WebSocket error response: {:?}",
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                            _ => {
                                // Use a new clone for unknown method
                                let ws_sender = Arc::clone(&ws_sender_arc);
                                let error_msg = format!("Unknown method: {}", request.method);
                                println!("{}", error_msg);

                                let mut sender = ws_sender.lock().await;
                                if let Err(e) = sender.send(Message::Text(error_msg)).await {
                                    println!("Error sending WebSocket error response: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // Use a new clone for error handling
                        let ws_sender = Arc::clone(&ws_sender_arc);
                        println!("Error parsing WebSocket request: {:?}", e);
                        let error_msg = format!("Invalid JSON format: {:?}", e);

                        let mut sender = ws_sender.lock().await;
                        if let Err(e) = sender.send(Message::Text(error_msg)).await {
                            println!("Error sending WebSocket error response: {:?}", e);
                            break;
                        }
                    }
                }
            }
        } else {
            println!("WebSocket connection closed");
            break;
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to the gRPC server
    let channel = tonic::transport::Channel::from_static("http://[::1]:50051")
        .connect()
        .await?;

    let client = EchoServiceClient::new(channel);

    // Start the WebSocket server
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = TcpListener::bind(&addr).await?;

    println!("WebSocket bridge listening on {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr()?;
        println!("WebSocket connection from: {}", peer);

        let client_clone = client.clone();
        tokio::spawn(async move {
            match accept_async(stream).await {
                Ok(websocket) => {
                    handle_websocket_connection(websocket, client_clone).await;
                }
                Err(e) => println!("Error during WebSocket handshake: {:?}", e),
            }
        });
    }

    Ok(())
}
