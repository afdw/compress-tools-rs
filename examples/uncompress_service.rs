use compress_tools::uncompress_data;
use std::net::TcpListener;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:1234")?;
    loop {
        let (socket, _) = listener.accept()?;
        println!("{:?}", uncompress_data(&socket, &socket));
    }
}
