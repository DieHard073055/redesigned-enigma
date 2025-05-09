// src/server.rs - A gRPC server implementation using Tonic for Rust

// Standard library imports
use std::pin::Pin; // Used for pinning data in memory which is required for async Streams
use std::time::{SystemTime, UNIX_EPOCH}; // For generating timestamps

// Tokio async runtime related imports
use tokio::sync::mpsc; // Multi-producer, single-consumer channel for async communication

// Stream handling imports
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt}; // Utilities for working with asynchronous streams
// ReceiverStream: Converts a tokio::sync::mpsc::Receiver into a Stream
// Stream: A trait representing an asynchronous sequence of values
// StreamExt: Extension methods for Stream

// Tonic (gRPC framework) imports
use tonic::{transport::Server, Request, Response, Status}; // Core components of Tonic gRPC
// Server: The gRPC server implementation
// Request: Wrapper around incoming gRPC requests
// Response: Wrapper around outgoing gRPC responses
// Status: gRPC status codes and error messages

// Import the generated code from protobuf definitions
use echo::echo_service_server::{EchoService, EchoServiceServer}; // Server traits generated from proto file
// EchoService: Trait that defines the service methods
// EchoServiceServer: Implementation of the server-side of the service

use echo::{EchoRequest, EchoResponse}; // Message types generated from proto file
// EchoRequest: Input message type defined in proto
// EchoResponse: Output message type defined in proto

// Include the generated protobuf code
// This macro includes code generated by tonic-build based on your .proto file
pub mod echo {
    tonic::include_proto!("echo"); // This expands to the code generated from the "echo" package in your proto file
}

// Implementation of our gRPC service
// #[derive(Debug, Default)]: Implements Debug trait for printing and Default for creating new instances
#[derive(Debug, Default)]
pub struct EchoServer {} // Our server struct - empty as we don't need state for this example

// Type alias for the response stream type used in streaming RPCs
// This is a complex type that represents:
// - Pin<Box<...>>: A heap-allocated, pinned value
// - dyn Stream<...>: A dynamic trait object implementing the Stream trait
// - Item = Result<EchoResponse, Status>: The stream yields Results containing either EchoResponses or Status errors
// - + Send: The stream can be sent across thread boundaries
type ResponseStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;

// Implement the EchoService trait for our EchoServer
// #[tonic::async_trait]: Macro that allows using async methods in traits
#[tonic::async_trait]
impl EchoService for EchoServer {
    // === UNARY RPC IMPLEMENTATION ===
    // This is the simplest form of RPC: client sends one request, server responds with one response
    async fn echo(&self, request: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        // Log the incoming request
        println!("Got a request: {:?}", request);

        // Extract the message from the request
        // into_inner() consumes the request and returns just the message payload
        let message = request.into_inner().message;

        // Create a timestamp for the response
        // SystemTime::now(): Gets the current system time
        // .duration_since(UNIX_EPOCH): Calculates the time elapsed since January 1, 1970
        // .unwrap(): Extracts the Ok result (or panics if an error occurred)
        // .as_secs(): Converts the duration to seconds
        // as i64: Casts the seconds to a 64-bit signed integer (required by our proto definition)
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Create the response object
        let reply = EchoResponse {
            message: format!("Echo: {}", message), // Format the reply message
            timestamp, // Include the timestamp
        };

        // Return the response wrapped in a Response object
        // The Ok() indicates success, and Response::new() wraps our reply
        Ok(Response::new(reply))
    }

    // === SERVER STREAMING IMPLEMENTATION ===
    // In this RPC type, the client sends one request, and the server responds with a stream of messages
    
    // Define the return type for the server_stream method
    // This must match the type defined in the protobuf service definition
    type ServerStreamStream = ResponseStream;

    async fn server_stream(
        &self,
        request: Request<EchoRequest>,
    ) -> Result<Response<Self::ServerStreamStream>, Status> {
        // Log the request
        println!("Server stream request: {:?}", request);

        // Extract the message from the request
        let message = request.into_inner().message;

        // Create a channel for sending responses
        // mpsc::channel(10): Creates a channel with a buffer size of 10 messages
        // tx: Transmitter (sender) side of the channel
        // rx: Receiver side of the channel
        let (tx, rx) = mpsc::channel(10);

        // Spawn a new asynchronous task to handle sending responses
        // This allows the server to continue processing other requests while streaming responses
        tokio::spawn(async move {
            // Send 5 responses with 1-second intervals
            for i in 1..=5 {
                // Create a timestamp for this response
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                // Create the response
                let reply = EchoResponse {
                    message: format!("Echo {}: {}", i, message), // Include the sequence number
                    timestamp,
                };

                // Send the response through the channel
                // tx.send(...).await.unwrap(): Send and await completion, panic if it fails
                // We wrap the response in Ok to match the Stream<Item = Result<...>> type
                tx.send(Ok(reply)).await.unwrap();

                // Wait 1 second before sending the next response
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
            // After sending all responses, the tx will be dropped when this task completes
            // This will signal to the receiver that no more messages are coming
        });

        // Convert the receiver to a stream and return it
        // ReceiverStream::new(rx): Converts the receiver to a Stream
        // Box::pin(...): Puts the stream on the heap and pins it
        // Response::new(...): Wraps the stream in a Response
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    // === CLIENT STREAMING IMPLEMENTATION ===
    // In this RPC type, the client sends multiple messages, and the server responds with a single message
    async fn client_stream(
        &self,
        request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<EchoResponse>, Status> {
        // Log the start of the client stream
        println!("Client stream started");

        // Extract the stream from the request
        // tonic::Streaming<EchoRequest> is a stream of incoming messages
        let mut stream = request.into_inner();

        // Initialize counters and storage for processing messages
        let mut message_count = 0;
        let mut combined_message = String::new();

        // Process all messages from the client
        // stream.message().await?: Waits for the next message and propagates errors
        // Some(req): Pattern matching to get the message if it exists
        // The loop continues until None is returned (end of stream)
        while let Some(req) = stream.message().await? {
            // Log each received message
            println!("Received client streaming message: {}", req.message);
            
            // Increment message counter
            message_count += 1;
            
            // Add a separator if this isn't the first message
            if !combined_message.is_empty() {
                combined_message.push_str(", ");
            }
            
            // Append this message to our accumulated string
            combined_message.push_str(&req.message);
        }

        // Log completion of the stream
        println!(
            "Client stream complete. Received {} messages.",
            message_count
        );

        // Create a timestamp for the response
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64; // Make sure this matches your proto definition type

        // Construct the response with a summary of all received messages
        let response = EchoResponse {
            message: format!("Received {} messages: {}", message_count, combined_message),
            timestamp,
        };

        // Log the response we're about to send
        println!("Sending client stream response: {:?}", response);
        
        // Return the response
        Ok(Response::new(response))
    }

    // === BIDIRECTIONAL STREAMING IMPLEMENTATION ===
    // In this RPC type, both client and server send multiple messages to each other
    
    // Define the return type for the bidirectional_stream method
    type BidirectionalStreamStream = ResponseStream;

    async fn bidirectional_stream(
        &self,
        request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<Self::BidirectionalStreamStream>, Status> {
        // Log the beginning of bidirectional streaming
        println!("Bidirectional stream request received");

        // Extract the stream of requests
        let mut stream = request.into_inner();

        // Create a channel for sending responses
        let (tx, rx) = mpsc::channel(10);

        // Spawn a new task to process incoming messages and send responses
        tokio::spawn(async move {
            // Process messages until the stream ends
            // stream.next().await: Gets the next message from the stream (returns Option<Result<...>>)
            while let Some(req) = stream.next().await {
                // Handle the result (which might be an error)
                match req {
                    // If we successfully received a message
                    Ok(echo_req) => {
                        // Log the received message
                        println!("Received message: {}", echo_req.message);

                        // Create a timestamp
                        let timestamp = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64;

                        // Create a response echoing back the message
                        let reply = EchoResponse {
                            message: format!("Echo: {}", echo_req.message),
                            timestamp,
                        };

                        // Send the response, and break the loop if sending fails
                        // This can happen if the client disconnects and the receiver is dropped
                        if tx.send(Ok(reply)).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    // If there was an error receiving the message
                    Err(err) => {
                        // Log the error and break the loop
                        println!("Error receiving message: {:?}", err);
                        break;
                    }
                }
            }
            // Log that the client has disconnected
            println!("Client disconnected");
            // The tx is dropped when this task completes, signaling the end of the response stream
        });

        // Return the receiver as a stream
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

// The main function - entry point of the program
// #[tokio::main]: Macro that sets up the Tokio runtime for async code
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse the address to listen on (IPv6 localhost, port 50051)
    let addr = "[::1]:50051".parse()?; // The ? operator propagates any errors
    
    // Create a new instance of our server
    let echo_server = EchoServer::default();
    
    // Log that the server is starting
    println!("gRPC server starting on {}", addr);
    
    // Create the gRPC service from our server implementation
    let service = EchoServiceServer::new(echo_server);
    
    // Enable gRPC-Web support for browser clients
    // This allows browsers to make gRPC calls using HTTP/1.1
    let grpc_web_service = tonic_web::enable(service);
    
    // Build and start the server
    Server::builder()
        .accept_http1(true) // Enable HTTP/1.1 for gRPC-Web
        .add_service(grpc_web_service) // Add our service to the server
        .serve(addr) // Start serving on the specified address
        .await?; // Wait for the server to complete and propagate any errors
    
    // Return Ok if everything completed successfully
    Ok(())
}
