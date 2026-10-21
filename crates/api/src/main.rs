use devchronicle_api::{serve, Config};

#[tokio::main]
async fn main() {
    init_tracing();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration error: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = serve(config).await {
        eprintln!("server error: {error}");
        std::process::exit(1);
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,sqlx=warn".into());
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
