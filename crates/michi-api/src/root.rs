use axum::response::Html;

pub async fn root_handler() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Michi Micro Server</title>
    <style>
        body { font-family: system-ui, -apple-system, sans-serif; display: flex; justify-content: center; align-items: center; min-height: 100vh; margin: 0; background: #1a1a2e; color: #e0e0e0; }
        .container { text-align: center; }
        h1 { color: #e94560; }
        .status { color: #4ecca3; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Michi Micro Server</h1>
        <p class="status">Server is running</p>
        <p>v0.1.0</p>
    </div>
</body>
</html>"#,
    )
}
