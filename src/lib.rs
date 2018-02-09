extern crate futures;
extern crate futures_cpupool;
extern crate tokio;
extern crate tokio_io;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_core;
extern crate bytes;

use std::io;
use std::str;
use bytes::BytesMut;
use tokio_io::codec::{Encoder, Decoder};
use tokio_proto::pipeline::ServerProto;
use tokio_service::Service;
use tokio_proto::TcpServer;

use tokio_core::reactor::Timeout;

use std::env;
use std::time::{Duration, Instant};
use std::net::SocketAddr;

use futures::prelude::*;
use futures::{Future, future};
use futures::future::{Executor, Map};
use futures::stream::Stream;
use futures_cpupool::CpuPool;
use tokio_io::io::copy;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use tokio::net::TcpListener;

/// Request from client
pub struct LineCodec;

impl Decoder for LineCodec {
    type Item = String;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<String>> {
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            // remove the serialized frame from the buffer.
            let line = buf.split_to(i);

            // Also remove the '\n'
            buf.split_to(1);

            // Turn this data into a UTF string and return it in a Frame.
            match str::from_utf8(&line) {
                Ok(s) => Ok(Some(s.to_string())),
                Err(_) => Err(io::Error::new(io::ErrorKind::Other,
                                             "invalid UTF-8")),
            }
        } else {
            Ok(None)
        }
    }
}

impl Encoder for LineCodec {
    type Item = String;
    type Error = io::Error;

    fn encode(&mut self, msg: String, buf: &mut BytesMut) -> io::Result<()> {
        buf.extend(msg.as_bytes());
        buf.extend(b"\n");
        Ok(())
    }
}

/// Protocol
pub struct LineProto;

impl<T: AsyncRead + AsyncWrite + 'static> ServerProto<T> for LineProto {
    // For this protocol style, `Request` matches the `Item` type of the codec's `Decoder`
    type Request = String;

    // For this protocol style, `Response` matches the `Item` type of the codec's `Encoder`
    type Response = String;

    // A bit of boilerplate to hook in the codec:
    type Transport = Framed<T, LineCodec>;
    type BindTransport = Result<Self::Transport, io::Error>;
    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(LineCodec))
    }
}


pub struct Echo;

impl Service for Echo {
    // These types must match the corresponding protocol types:
    type Request = String;
    type Response = String;

    // For non-streaming protocols, service errors are always io::Error
    type Error = io::Error;

    // The future for computing the response; box it for simplicity.
    type Future = Box<Future<Item = Self::Response, Error =  Self::Error>>;

    // Produce a future for computing a response from a request.
    fn call(&self, req: Self::Request) -> Self::Future {
        // In this case, the response is immediate.
        let mut resp: String = "\n".to_owned();
        resp.push_str(&req);
        Box::new(future::ok(resp))
    }
}


pub fn run_echoserver(ip_port: String) {

    // let mut ip_port: String = "".to_owned();

    // ip_port.push_str(&ip);
    // ip_port.push_str(":");
    // ip_port.push_str(&port);

    // Specify the localhost address
    let addr = ip_port.parse().unwrap();

    // The builder requires a protocol and an address
    let server = TcpServer::new(LineProto, addr);

    // We provide a way to *instantiate* the service for each new
    // connection; here, we just immediately return a new instance.
    server.serve(|| Ok(Echo));
}


fn run_server(thresh_time: u64, time_out: u64, count: usize) {
    // Allow passing an address to listen on as the first argument of this
    // program, but otherwise we'll just set up our TCP listener on
    // 127.0.0.1:8080 for connections.
    let addr = env::args().nth(1).unwrap_or("127.0.0.1:8080".to_string());
    let addr = addr.parse::<SocketAddr>().unwrap();

    // Next up we create a TCP listener which will listen for incoming
    // connections. This TCP listener is bound to the address we determined
    // above and must be associated with an event loop, so we pass in a handle
    // to our event loop. After the socket's created we inform that we're ready
    // to go and start accepting connections.
    let socket = TcpListener::bind(&addr).unwrap();
    println!("Listening on: {}", addr);

    // A CpuPool allows futures to be executed concurrently.
    let pool = CpuPool::new(1);

    // Here we convert the `TcpListener` to a stream of incoming connections
    // with the `incoming` method. We then define how to process each element in
    // the stream with the `for_each` method.
    //
    // This combinator, defined on the `Stream` trait, will allow us to define a
    // computation to happen for all items on the stream (in this case TCP
    // connections made to the server).  The return value of the `for_each`
    // method is itself a future representing processing the entire stream of
    // connections, and ends up being our server.
    let done = socket.incoming().for_each(move |socket| {

        // Once we're inside this closure this represents an accepted client
        // from our server. The `socket` is the client connection (similar to
        // how the standard library operates).
        //
        // We just want to copy all data read from the socket back onto the
        // socket itself (e.g. "echo"). We can use the standard `io::copy`
        // combinator in the `tokio-core` crate to do precisely this!
        //
        // The `copy` function takes two arguments, where to read from and where
        // to write to. We only have one argument, though, with `socket`.
        // Luckily there's a method, `Io::split`, which will split an Read/Write
        // stream into its two halves. This operation allows us to work with
        // each stream independently, such as pass them as two arguments to the
        // `copy` function.
        //
        // The `copy` function then returns a future, and this future will be
        // resolved when the copying operation is complete, resolving to the
        // amount of data that was copied.
        let (reader, writer) = socket.framed(LineCodec).split();
        // let amt = copy(reader, writer);

        // After our copy operation is complete we just print out some helpful
        // information.
        // let msg = amt.then(move |result| {
        //     match result {
        //         Ok((amt, _, _)) => println!("wrote {} bytes", amt),
        //         Err(e) => println!("error: {}", e),
        //     }

        //     Ok(())
        // });

        let counting = move || {
            let now = Instant::now();
            for _ in 0..count {}
            let new_now = Instant::now();
            Some(new_now.duration_since(now))
        }.map(Ok);

        let timeout = timeout(Duration::new(time_out, 0)).map(Err);

        let task = counting.select(timeout).then(|result| {
            match result {
                // One future succeeded, and it was the one which was
                // downloading data from the connection.
                Ok((Some(time_count), _other_future)) => {

                    if time_count.secs < thresh_time {
                        writer.write_all(b"Error: counting is less than thresh!\n")                         
                    } 

                    if time_count.secs > thresh_time || (time_count.secs == thresh_time && time_count.nanos > 0) {
                        writer.write_fmt(format_args!("Time elapsed is {} sec {} ns.\n", time_count.secs, time_count.nanos))                        
                    } 

                    Ok(())
                }

                // The timeout fired, and otherwise no error was found, so
                // we translate this to an error.
                Ok((Err(_timeout), _other_future)) => {
                    Err(io::Error::new(io::ErrorKind::Other, "timeout"))
                }

                // A normal I/O error happened, so we pass that on through.
                Err((e, _other_future)) => Err(e),
            }
        });

        // And this is where much of the magic of this server happens. We
        // crucially want all clients to make progress concurrently, rather than
        // blocking one on completion of another. To achieve this we use the
        // `execute` function on the `Executor` trait to essentially execute
        // some work in the background.
        //
        // This function will transfer ownership of the future (`msg` in this
        // case) to the event loop that `handle` points to. The event loop will
        // then drive the future to completion.
        //
        // Essentially here we're executing a new task to run concurrently,
        // which will allow all of our clients to be processed concurrently.
        pool.execute(task).unwrap();

        Ok(())
    });

    // And finally now that we've define what our server is, we run it! Here we
    // just need to execute the future we've created and wait for it to complete
    // using the standard methods in the `futures` crate.
    done.wait().unwrap();
}


// Example with winner of future
// let data = data.map(Ok);
// let timeout = timeout(timeout_dur).map(Err);

// let ret = data.select(timeout).then(|result| {
//     match result {
//         // One future succeeded, and it was the one which was
//         // downloading data from the connection.
//         Ok((Ok(data), _other_future)) => Ok(data),

//         // The timeout fired, and otherwise no error was found, so
//         // we translate this to an error.
//         Ok((Err(_timeout), _other_future)) => {
//             Err(io::Error::new(io::ErrorKind::Other, "timeout"))
//         }

//         // A normal I/O error happened, so we pass that on through.
//         Err((e, _other_future)) => Err(e),
//     }
// });
// return Box::new(ret);