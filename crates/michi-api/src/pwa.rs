pub async fn manifest_json() -> impl axum::response::IntoResponse {
    ([("content-type", "application/json")], MANIFEST_JSON)
}

pub async fn sw_js() -> impl axum::response::IntoResponse {
    ([("content-type", "application/javascript")], SW_JS)
}

const MANIFEST_JSON: &str = r##"{
  "name": "Michi Micro Server",
  "short_name": "Michi",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#08070d",
  "theme_color": "#090711",
  "icons": [
    {
      "src": "/static/assets/michi-logo.svg",
      "sizes": "any",
      "type": "image/svg+xml",
      "purpose": "any maskable"
    }
  ]
}"##;

const SW_JS: &str = r#"const CACHE = 'michi-v4';

self.addEventListener('install', function(e) {
  e.waitUntil(
    caches.open(CACHE).then(function(c) {
      return c.addAll(['/']);
    })
  );
});

self.addEventListener('activate', function(e) {
  e.waitUntil(
    caches.keys().then(function(keys) {
      return Promise.all(
        keys.filter(function(k) { return k !== CACHE; })
            .map(function(k) { return caches.delete(k); })
      );
    })
  );
});

self.addEventListener('fetch', function(e) {
  var url = new URL(e.request.url);
  if (url.pathname === '/') {
    e.respondWith(networkFirst(e.request));
  } else if (url.pathname.startsWith('/api/')) {
    e.respondWith(fetch(e.request));
  } else {
    e.respondWith(networkFirst(e.request));
  }
});

function networkFirst(req) {
  return fetch(req).then(function(resp) {
    if (resp.ok && req.method === 'GET') {
      var clone = resp.clone();
      caches.open(CACHE).then(function(ca) {
        ca.put(req, clone);
      });
    }
    return resp;
  }).catch(function() {
    return caches.match(req);
  });
}"#;
