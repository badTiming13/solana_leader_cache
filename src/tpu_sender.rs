use anyhow::Result;
use tokio::net::UdpSocket;

/// Отправляем raw tx bytes на UDP TPU endpoint "ip:port"
pub async fn send_udp_tx(tx_bytes: &[u8], tpu_addr: &str) -> Result<()> {
    // bind ephemeral local port
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.send_to(tx_bytes, tpu_addr).await?;
    Ok(())
}

/// Отправляем в несколько endpoints (current + next leaders)
pub async fn send_udp_tx_multi(tx_bytes: &[u8], addrs: &[String]) -> Result<()> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    for a in addrs {
        let _ = sock.send_to(tx_bytes, a).await; // best-effort
    }
    Ok(())
}
