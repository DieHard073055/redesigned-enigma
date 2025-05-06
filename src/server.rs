// src/server.rs
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{transport::Server, Request, Response, Status};

// Import generated protobuf code
use echo::echo_service_server::{EchoService, EchoServiceServer};
use echo::{EchoRequest, EchoResponse};

// Include the generated protobuf code
pub mod echo {
    tonic::include_proto!("echo");
}

// Implementation of our gRPC service
#[derive(Debug, Default)]
pub struct EchoServer {}

type ResponseStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;

#[tonic::async_trait]
impl EchoService for EchoServer {
    // Unary RPC implementation
    async fn echo(&self, request: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        println!("Got a request: {:?}", request);

        let message = request.into_inner().message;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let reply = EchoResponse {
            message: format!("Echo: {}", message),
            timestamp,
        };

        Ok(Response::new(reply))
    }

    // Server streaming implementation
    type ServerStreamStream = ResponseStream;

    async fn server_stream(
        &self,
        request: Request<EchoRequest>,
    ) -> Result<Response<Self::ServerStreamStream>, Status> {
        println!("Server stream request: {:?}", request);

        let message = request.into_inner().message;
        let (tx, rx) = mpsc::channel(10);

        // Send 5 responses
        tokio::spawn(async move {
            for i in 1..=5 {
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                let reply = EchoResponse {
                    message: format!("Echo {}: {}", i, message),
                    timestamp,
                };

                tx.send(Ok(reply)).await.unwrap();
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    // Client streaming implementation
    async fn client_stream(
        &self,
        request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<EchoResponse>, Status> {
        println!("Client stream started");

        let mut stream = request.into_inner();
        let mut message_count = 0;
        let mut combined_message = String::new();

        // Process all messages from the client
        while let Some(req) = stream.message().await? {
            println!("Received client streaming message: {}", req.message);
            message_count += 1;
            if !combined_message.is_empty() {
                combined_message.push_str(", ");
            }
            combined_message.push_str(&req.message);
        }

        println!(
            "Client stream complete. Received {} messages.",
            message_count
        );

        // Construct and return the response with proper timestamp type
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64; // Make sure this matches your proto definition type

        let response = EchoResponse {
            message: format!("Received {} messages: {}", message_count, combined_message),
            timestamp, // Using the correctly typed timestamp
        };

        println!("Sending client stream response: {:?}", response);
        Ok(Response::new(response))
    }
    // Bidirectional streaming (WebSocket-like) implementation
    type BidirectionalStreamStream = ResponseStream;

    async fn bidirectional_stream(
        &self,
        request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<Self::BidirectionalStreamStream>, Status> {
        println!("Bidirectional stream request received");

        let mut stream = request.into_inner();
        let (tx, rx) = mpsc::channel(10);

        // Echo back each message with a timestamp
        tokio::spawn(async move {
            while let Some(req) = stream.next().await {
                match req {
                    Ok(echo_req) => {
                        println!("Received message: {}", echo_req.message);

                        let timestamp = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64;

                        let reply = EchoResponse {
                            message: format!("Echo: {}", echo_req.message),
                            timestamp,
                        };

                        if tx.send(Ok(reply)).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    Err(err) => {
                        println!("Error receiving message: {:?}", err);
                        break;
                    }
                }
            }
            println!("Client disconnected");
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let echo_server = EchoServer::default();

    println!("gRPC server starting on {}", addr);

    // Add gRPC-Web support for browser clients
    let service = EchoServiceServer::new(echo_server);
    let grpc_web_service = tonic_web::enable(service);

    Server::builder()
        .accept_http1(true) // Enable HTTP/1.1 for gRPC-Web
        .add_service(grpc_web_service)
        .serve(addr)
        .await?;

    Ok(())
}
