use testcontainers::{clients::Cli, Container, GenericImage, RunnableImage};

pub fn spawn_clickhouse(docker: &Cli) -> (Container<'_, GenericImage>, u16, String) {
    let image = GenericImage::new("clickhouse/clickhouse-server", "23.3")
        .with_exposed_port(8123)
        .with_exposed_port(9000);

    let runnable = RunnableImage::from(image);
    
    let container = docker.run(runnable);
    
    let http_port = container.get_host_port_ipv4(8123);
    
    let http_url = format!("http://localhost:{}", http_port);
    
    (container, http_port, http_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_clickhouse() {
        let docker = Cli::default();
        let (_container, port, url) = spawn_clickhouse(&docker);
        
        assert!(port > 0, "port should be assigned");
        assert!(url.starts_with("http://localhost:"), "url should be valid");
        assert!(url.contains(&port.to_string()), "url should contain the port");
    }
}
