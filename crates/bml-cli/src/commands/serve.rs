use std::path::{Path, PathBuf};
use std::sync::Arc;

use bml_core::codegen;
use bml_core::parser::cst_to_ast;
use bml_core::scene;
use bml_core::semantic;
use bml_core::semantic::diagnostics::Severity;
use futures_util::StreamExt;

pub fn run(
    port: u16,
    bind: String,
    web_dir: PathBuf,
    examples_dir: Option<PathBuf>,
) -> miette::Result<()> {
    let web_dir = Arc::new(web_dir);

    if !web_dir.join("index.html").is_file() {
        return Err(miette::miette!(
            "web directory {} does not contain index.html",
            web_dir.display()
        ));
    }

    let examples_dir = examples_dir.map(Arc::new);

    eprintln!(
        "\x1b[32mStarting BuildingML server\x1b[0m on http://{}:{}",
        bind, port,
    );
    eprintln!("  Web App:  http://{}:{}/", bind, port);
    eprintln!("  Health:   http://{}:{}/health", bind, port);
    eprintln!("  API:      POST /api/scene, /api/lint, /api/model, /api/compile, /api/chat");
    if let Some(ref dir) = examples_dir {
        eprintln!(
            "  Examples: http://{}:{}/api/examples (from {})",
            bind,
            port,
            dir.display()
        );
    }
    eprintln!("Press Ctrl+C to stop.");

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(serve(port, bind, web_dir, examples_dir))
}

async fn serve(
    port: u16,
    bind: String,
    web_dir: Arc<PathBuf>,
    examples_dir: Option<Arc<PathBuf>>,
) -> miette::Result<()> {
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind, port))
        .await
        .map_err(|e| miette::miette!("failed to bind to {}:{}: {}", bind, port, e))?;

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| miette::miette!("accept error: {}", e))?;

        let web_dir = web_dir.clone();
        let examples_dir = examples_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &web_dir, examples_dir.as_deref()).await {
                eprintln!("  connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    web_dir: &Path,
    examples_dir: Option<&PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let path = parts[1];

    // Read all headers to find Content-Length
    let mut content_length: usize = 0;
    loop {
        let mut header_line = String::new();
        reader.read_line(&mut header_line).await?;
        if header_line.trim().is_empty() {
            break;
        }
        let lower = header_line.to_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }

    // Read body for POST requests
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        use tokio::io::AsyncReadExt;
        reader.read_exact(&mut body).await?;
    }
    let body_str = String::from_utf8_lossy(&body);

    let stream = reader.into_inner();

    match (method, path) {
        // --- Health check ---
        ("GET", "/health") => send_response(stream, 200, "text/plain", b"OK").await,

        // --- POST-only API routes ---
        ("POST", "/api/scene") => {
            if body_str.is_empty() {
                return send_response(stream, 400, "text/plain", b"Request body required").await;
            }
            match generate_scene_json(&body_str, examples_dir.map(|p| p.as_path())) {
                Ok(json) => send_response(stream, 200, "application/json", json.as_bytes()).await,
                Err(e) => send_response(stream, 400, "text/plain", e.as_bytes()).await,
            }
        }
        ("POST", "/api/lint") => {
            if body_str.is_empty() {
                return send_response(stream, 400, "text/plain", b"Request body required").await;
            }
            let json = lint_source(&body_str);
            send_response(stream, 200, "application/json", json.as_bytes()).await
        }
        ("POST", "/api/model") => {
            if body_str.is_empty() {
                return send_response(stream, 400, "text/plain", b"Request body required").await;
            }
            match generate_model_json(&body_str, examples_dir.map(|p| p.as_path())) {
                Ok(json) => send_response(stream, 200, "application/json", json.as_bytes()).await,
                Err(e) => send_response(stream, 400, "text/plain", e.as_bytes()).await,
            }
        }
        ("POST", "/api/compile") => {
            if body_str.is_empty() {
                return send_response(stream, 400, "text/plain", b"Request body required").await;
            }
            match compile_to_ifc(&body_str) {
                Ok(ifc) => send_response(stream, 200, "text/plain", ifc.as_bytes()).await,
                Err(e) => send_response(stream, 400, "text/plain", e.as_bytes()).await,
            }
        }

        // --- Chat API route (OpenAI-compatible proxy) ---
        ("POST", "/api/chat") => {
            if body_str.is_empty() {
                return send_response(stream, 400, "text/plain", b"Request body required").await;
            }
            handle_chat(stream, &body_str).await
        }

        // --- Example API routes ---
        ("GET", "/api/examples") => {
            if let Some(dir) = examples_dir {
                let json = list_examples(dir);
                send_response(stream, 200, "application/json", json.as_bytes()).await
            } else {
                send_response(stream, 200, "application/json", b"[]").await
            }
        }
        ("GET", p) if p.starts_with("/api/examples/") => {
            if let Some(dir) = examples_dir {
                let filename = &p["/api/examples/".len()..];
                serve_example(stream, dir, filename).await
            } else {
                send_response(
                    stream,
                    404,
                    "text/plain",
                    b"No examples directory configured",
                )
                .await
            }
        }

        // --- CORS preflight ---
        ("OPTIONS", _) => send_response(stream, 200, "text/plain", b"").await,

        // --- Static file serving (React app) ---
        ("GET", _) => serve_static_file(stream, web_dir, path).await,

        _ => send_response(stream, 404, "text/plain", b"Not Found").await,
    }
}

/// Serve a static file from the web directory.
/// For SPA routing, returns index.html for paths that don't match a real file.
async fn serve_static_file(
    stream: tokio::net::TcpStream,
    web_dir: &Path,
    request_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let clean_path = request_path.trim_start_matches('/');
    let file_path = if clean_path.is_empty() {
        web_dir.join("index.html")
    } else {
        web_dir.join(clean_path)
    };

    // Security: prevent path traversal (canonicalize only works on existing files)
    if file_path.is_file() {
        let canonical = file_path.canonicalize().unwrap_or_default();
        let web_canonical = web_dir.canonicalize().unwrap_or_default();
        if !canonical.starts_with(&web_canonical) {
            return send_response(stream, 403, "text/plain", b"Forbidden").await;
        }
    }

    if file_path.is_file() {
        let content = std::fs::read(&file_path)?;
        let mime = guess_mime(&file_path);
        send_response(stream, 200, mime, &content).await
    } else {
        // SPA fallback: serve index.html for client-side routing
        let index = web_dir.join("index.html");
        if index.is_file() {
            let content = std::fs::read(&index)?;
            send_response(stream, 200, "text/html", &content).await
        } else {
            send_response(stream, 404, "text/plain", b"Not Found").await
        }
    }
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("woff") | Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn list_examples(dir: &Path) -> String {
    let mut examples: Vec<serde_json::Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "bml"))
            .collect();
        files.sort_by_key(|e| e.file_name());
        for entry in files {
            let file = entry.file_name().to_string_lossy().to_string();
            let name = file.trim_end_matches(".bml").replace('_', " ");
            examples.push(serde_json::json!({ "name": name, "file": file }));
        }
    }
    serde_json::to_string(&examples).unwrap_or_else(|_| "[]".to_string())
}

async fn serve_example(
    stream: tokio::net::TcpStream,
    dir: &Path,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Security: only allow .bml files, no path traversal
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return send_response(stream, 403, "text/plain", b"Forbidden").await;
    }
    if !filename.ends_with(".bml") {
        return send_response(stream, 400, "text/plain", b"Only .bml files allowed").await;
    }
    let file_path = dir.join(filename);
    if !file_path.is_file() {
        return send_response(stream, 404, "text/plain", b"Example not found").await;
    }
    match std::fs::read_to_string(&file_path) {
        Ok(content) => send_response(stream, 200, "text/plain", content.as_bytes()).await,
        Err(e) => send_response(stream, 500, "text/plain", e.to_string().as_bytes()).await,
    }
}

fn generate_scene_json(source: &str, base_dir: Option<&std::path::Path>) -> Result<String, String> {
    let model = analyze_source(source, base_dir)?;
    let scene_graph = scene::generate_scene(&model);
    serde_json::to_string(&scene_graph).map_err(|e| format!("serialization error: {}", e))
}

fn generate_model_json(source: &str, base_dir: Option<&std::path::Path>) -> Result<String, String> {
    let model = analyze_source(source, base_dir)?;
    serde_json::to_string(&model).map_err(|e| format!("serialization error: {}", e))
}

/// Parse and analyze BML source, resolving imports via a temp file when base_dir is available.
fn analyze_source(
    source: &str,
    base_dir: Option<&std::path::Path>,
) -> Result<bml_core::semantic::model::SemanticModel, String> {
    let has_imports = source
        .lines()
        .any(|l| l.trim_start().starts_with("import "));

    if has_imports && let Some(dir) = base_dir {
        let tmp_name = format!(
            ".bml-serve-{}.bml",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        );
        let tmp_path = dir.join(&tmp_name);

        if std::fs::write(&tmp_path, source).is_ok() {
            let result = bml_core::project::Project::load_entry_file(&tmp_path)
                .map_err(|e| e.to_string())
                .and_then(|project| {
                    let (model, diagnostics) = project.analyze();
                    check_errors(&diagnostics)?;
                    Ok(model)
                });
            let _ = std::fs::remove_file(&tmp_path);
            return result;
        }
    }

    let ast = parse_source(source)?;
    let (model, diagnostics) = semantic::analyze(&ast);
    check_errors(&diagnostics)?;
    Ok(model)
}

fn compile_to_ifc(source: &str) -> Result<String, String> {
    let ast = parse_source(source)?;
    let (model, diagnostics) = semantic::analyze(&ast);
    check_errors(&diagnostics)?;

    codegen::generate_ifc(&model).map_err(|e| format!("codegen error: {}", e))
}

fn parse_source(source: &str) -> Result<bml_core::ast::nodes::SourceFile, String> {
    cst_to_ast::parse(source).map_err(|errors| {
        format!(
            "Parse error(s): {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        )
    })
}

fn check_errors(diagnostics: &[bml_core::semantic::diagnostics::Diagnostic]) -> Result<(), String> {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        return Err(format!(
            "{} error(s): {}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("[{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    Ok(())
}

fn lint_source(source: &str) -> String {
    let ast = match cst_to_ast::parse(source) {
        Ok(ast) => ast,
        Err(errors) => {
            let parse_errors: Vec<serde_json::Value> = errors
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "severity": "error",
                        "message": e.to_string(),
                        "code": "E001",
                    })
                })
                .collect();
            return serde_json::to_string(&parse_errors).unwrap_or_else(|_| "[]".to_string());
        }
    };

    let (_, diagnostics) = semantic::analyze(&ast);

    let diag_json: Vec<serde_json::Value> = diagnostics
        .iter()
        .map(|d| {
            serde_json::json!({
                "code": d.code.as_str(),
                "severity": d.severity_str(),
                "message": d.message,
                "line": d.span.start.line,
                "column": d.span.start.column,
                "help": d.help,
            })
        })
        .collect();

    serde_json::to_string(&diag_json).unwrap_or_else(|_| "[]".to_string())
}

/// Proxy chat requests to an OpenAI-compatible API endpoint.
/// Reads configuration from environment variables:
///   OPENAI_API_KEY — API key for authentication
///   OPENAI_BASE_URL — Base URL (default: https://api.openai.com/v1)
///   OPENAI_MODEL — Model name (default: gpt-4o)
async fn handle_chat(
    mut stream: tokio::net::TcpStream,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncWriteExt;

    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        return send_response(
            stream,
            500,
            "text/plain",
            b"OPENAI_API_KEY environment variable not set",
        )
        .await;
    }

    let base_url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

    // Parse the incoming request body
    let request: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return send_response(
                stream,
                400,
                "text/plain",
                format!("Invalid JSON: {}", e).as_bytes(),
            )
            .await;
        }
    };

    let messages = request
        .get("messages")
        .cloned()
        .unwrap_or(serde_json::json!([]));

    // Build OpenAI-compatible request
    let openai_request = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let response = match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&openai_request)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return send_response(
                stream,
                502,
                "text/plain",
                format!("Failed to connect to LLM API: {}", e).as_bytes(),
            )
            .await;
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let error_body = response.text().await.unwrap_or_default();
        return send_response(
            stream,
            status,
            "text/plain",
            format!("LLM API error: {}", error_body).as_bytes(),
        )
        .await;
    }

    // Stream the response back as SSE (Server-Sent Events)
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
    stream.write_all(header.as_bytes()).await?;

    let mut byte_stream = response.bytes_stream();
    while let Some(chunk) = byte_stream.next().await {
        match chunk {
            Ok(bytes) => {
                // Write chunked transfer encoding
                let hex_len = format!("{:x}\r\n", bytes.len());
                stream.write_all(hex_len.as_bytes()).await?;
                stream.write_all(&bytes).await?;
                stream.write_all(b"\r\n").await?;
                stream.flush().await?;
            }
            Err(e) => {
                eprintln!("  chat stream error: {}", e);
                break;
            }
        }
    }

    // Final chunk
    stream.write_all(b"0\r\n\r\n").await?;
    stream.flush().await?;
    Ok(())
}

async fn send_response(
    mut stream: tokio::net::TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncWriteExt;

    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };

    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
        status,
        status_text,
        content_type,
        body.len()
    );

    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}
