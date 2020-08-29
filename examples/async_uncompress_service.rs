use compress_tools::tokio_support::uncompress_data;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut listener = TcpListener::bind("127.0.0.1:1234").await?;
    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (read_half, write_half) = socket.into_split();
            println!("{:?}", uncompress_data(read_half, write_half).await);
        });
    }
}
