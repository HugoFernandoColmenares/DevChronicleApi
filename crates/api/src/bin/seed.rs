use devchronicle_api::{
    database_url_from_env, prepare_database, resolve_content_dir, seed_from_content,
};

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,sqlx=warn".into());
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let database_url = match database_url_from_env() {
        Ok(url) => url,
        Err(error) => {
            eprintln!("configuration error: {error}");
            std::process::exit(1);
        }
    };

    let content_dir = match resolve_content_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("seed error: {error}");
            std::process::exit(1);
        }
    };

    let pool = match prepare_database(&database_url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("database error: {error}");
            std::process::exit(1);
        }
    };

    match seed_from_content(&pool, &content_dir).await {
        Ok(report) => {
            println!(
                "seed complete: inserted {}, skipped {} (from {})",
                report.inserted,
                report.skipped,
                content_dir.display()
            );
        }
        Err(error) => {
            eprintln!("seed error: {error}");
            std::process::exit(1);
        }
    }
}
