import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        title = {"/a.html": "Alpha page", "/b.html": "Beta page"}.get(self.path, "Other")
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        if self.path == "/a.html":
            self.send_header("Set-Cookie", "sid=hello123; Max-Age=86400; Path=/; HttpOnly; SameSite=Lax")
        self.end_headers()
        self.wfile.write(f"<!doctype html><title>{title}</title><p>{title}".encode())
    def log_message(self, *a): pass
http.server.HTTPServer(("127.0.0.1", 8767), H).serve_forever()
