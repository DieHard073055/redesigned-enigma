// src/client.rs
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;

// Import generated protobuf code
use echo::echo_service_client::EchoServiceClient;
use echo::EchoRequest;

// Include the generated protobuf code
pub mod echo {
    tonic::include_proto!("echo");
}

async fn unary_call(
    client: &mut EchoServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = tonic::Request::new(EchoRequest {
        message: String::from("Hello from client"),
    });

    let response = client.echo(request).await?;
    println!("RESPONSE: {:?}", response.into_inner());

    Ok(())
}

async fn server_streaming(
    client: &mut EchoServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = tonic::Request::new(EchoRequest {
        message: String::from("Stream me some responses"),
    });

    let mut stream = client.server_stream(request).await?.into_inner();

    while let Some(response) = stream.next().await {
        match response {
            Ok(msg) => println!("SERVER STREAM RESPONSE: {:?}", msg),
            Err(e) => println!("SERVER STREAM ERROR: {:?}", e),
        }
    }

    Ok(())
}

async fn client_streaming(
    client: &mut EchoServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting client streaming");
    
    // Create channel with buffer
    let (tx, rx) = mpsc::channel(10);
    
    // Create stream from receiver
    let stream = ReceiverStream::new(rx);
    
    // Create request with stream
    let request = tonic::Request::new(stream);
    
    // Start the request (important to do this before sending messages)
    let response_future = client.client_stream(request);
    
    // Now send messages
    for i in 1..=5 {
        let msg = EchoRequest {
            message: format!("Client message {}", i),
        };
        println!("Sending message {}", i);
        tx.send(msg).await?;
        println!("Sent client message {}", i);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    // Drop the sender to signal end of stream
    println!("Dropping sender to signal end of stream");
    drop(tx);
    
    // Wait for the response
    println!("Waiting for response...");
    let response = response_future.await?;
    println!("CLIENT STREAM RESPONSE: {:?}", response.into_inner());
    
    Ok(())
}


async fn bidirectional_streaming(
    client: &mut EchoServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel(4);
    let stream = ReceiverStream::new(rx);
    let request = tonic::Request::new(stream);

    let mut response_stream = client.bidirectional_stream(request).await?.into_inner();

    // Create a task to send messages
    let sender_task = tokio::spawn(async move {
        for i in 1..=5 {
            let msg = EchoRequest {
                message: format!("Bidirectional message {}", i),
            };
            match tx.send(msg).await {
                Ok(_) => println!("Sent message {}", i),
                Err(e) => {
                    println!("Failed to send message: {:?}", e);
                    break;
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    });

    // Process responses
    let receiver_task = tokio::spawn(async move {
        while let Some(response) = response_stream.next().await {
            match response {
                Ok(msg) => println!("BIDIRECTIONAL RESPONSE: {:?}", msg),
                Err(e) => {
                    println!("BIDIRECTIONAL STREAM ERROR: {:?}", e);
                    break;
                }
            }
        }
    });

    // Wait for both tasks
    tokio::try_join!(sender_task, receiver_task)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = tonic::transport::Channel::from_static("http://[::1]:50051")
        .connect()
        .await?;

    let mut client = EchoServiceClient::new(channel);

    println!("\n=== Unary Call ===");
    unary_call(&mut client).await?;

    println!("\n=== Server Streaming ===");
    server_streaming(&mut client).await?;

    println!("\n=== Client Streaming ===");
    client_streaming(&mut client).await?;

    println!("\n=== Bidirectional Streaming (WebSocket-like) ===");
    bidirectional_streaming(&mut client).await?;

    Ok(())
}
