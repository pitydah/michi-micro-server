use axum::response::{IntoResponse, Response};
use std::sync::LazyLock;

pub async fn manifest_json() -> impl IntoResponse {
    ([("content-type", "application/json")], MANIFEST_JSON)
}

static SW_JS: LazyLock<String> = LazyLock::new(|| {
    let v = crate::assets::asset_version();
    format!(
        r#"const CACHE = 'michi-{v}';

self.addEventListener('install', function(e) {{
  self.skipWaiting();
  e.waitUntil(
    caches.open(CACHE).then(function(c) {{
      return c.addAll([
        '/',
        '/static/styles.css?v={v}',
        '/static/hero-cat.css?v={v}',
        '/static/app.js?v={v}',
        '/static/assets/michi-hero-cat.webp?v={v}',
        '/static/assets/michi-micro-server.svg',
        '/static/assets/michi-micro-server-180.png',
        '/static/assets/michi-micro-server-192.png',
        '/static/assets/michi-micro-server-512.png'
      ]);
    }})
  );
}});

self.addEventListener('activate', function(e) {{
  e.waitUntil(
    caches.keys().then(function(keys) {{
      return Promise.all(
        keys.filter(function(k) {{ return k !== CACHE; }})
            .map(function(k) {{ return caches.delete(k); }})
      );
    }}).then(function() {{
      return self.clients.claim();
    }})
  );
}});

self.addEventListener('fetch', function(e) {{
  var url = new URL(e.request.url);
  if (url.pathname === '/' || url.pathname === '/index.html') {{
    e.respondWith(networkFirst(e.request));
  }} else if (url.pathname.startsWith('/api/')) {{
    e.respondWith(fetch(e.request));
  }} else {{
    e.respondWith(networkFirst(e.request));
  }}
}});

function networkFirst(req) {{
  return fetch(req).then(function(resp) {{
    if (resp.ok && req.method === 'GET') {{
      var clone = resp.clone();
      caches.open(CACHE).then(function(ca) {{
        ca.put(req, clone);
      }});
    }}
    return resp;
  }}).catch(function() {{
    return caches.match(req);
  }});
}}
"#
    )
});

pub fn sw_js_content() -> &'static str {
    &SW_JS
}

pub async fn sw_js() -> impl IntoResponse {
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .header("cache-control", "no-cache, no-store, must-revalidate")
        .header("pragma", "no-cache")
        .header("expires", "0")
        .body(axum::body::Body::from(SW_JS.as_str()))
        .unwrap()
}

const MANIFEST_JSON: &str = r##"{
  "name": "Michi Micro Server",
  "short_name": "Michi",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#090B10",
  "theme_color": "#090B10",
  "icons": [
    {
      "src": "/static/assets/michi-micro-server-192.png",
      "sizes": "192x192",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/static/assets/michi-micro-server-512.png",
      "sizes": "512x512",
      "type": "image/png",
      "purpose": "any"
    }
  ]
}"##;
