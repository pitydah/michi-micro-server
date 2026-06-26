pub async fn manifest_json() -> impl axum::response::IntoResponse {
    ([("content-type", "application/json")], MANIFEST_JSON)
}

pub async fn sw_js() -> impl axum::response::IntoResponse {
    ([("content-type", "application/javascript")], SW_JS)
}

const MANIFEST_JSON: &str = r##"{"name":"Michi Micro Server","short_name":"Michi","start_url":"/","display":"standalone","background_color":"#1a1a2e","theme_color":"#e94560"}"##;

const SW_JS: &str = r#"const C='michi-v1';
self.addEventListener('install',function(e){e.waitUntil(caches.open(C).then(function(c){return c.addAll(['/'])}))});
self.addEventListener('activate',function(e){e.waitUntil(caches.keys().then(function(k){return Promise.all(k.filter(function(x){return x!==C}).map(function(x){return caches.delete(x)}))}))});
self.addEventListener('fetch',function(e){var u=new URL(e.request.url);if(u.pathname==='/'){e.respondWith(networkFirst(e.request))}else if(u.pathname.startsWith('/api/')){e.respondWith(fetch(e.request))}else{e.respondWith(networkFirst(e.request))}});
function networkFirst(r){return fetch(r).then(function(resp){if(resp.ok&&r.method==='GET'){var c=resp.clone();caches.open(C).then(function(ca){ca.put(r,c)})}return resp}).catch(function(){return caches.match(r)})}"#;
