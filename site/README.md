# Inert website

Plain static HTML: `index.html` (Browser, blue) and `protocol.html` (Protocol,
dark), screenshots in `shots/`, the icon from `inert-browser --write-icon`.
No build step and no scripts beyond the page itself.

## Self-hosting

`docker-compose.yml` runs Caddy for the files and a Cloudflare Tunnel in front
of it, so no port has to be opened. Copy the site and the two config files to
a server (`HOST` is yours):

    rsync -a --delete index.html protocol.html icon.png shots HOST:~/inert-site/public/
    rsync -a Caddyfile docker-compose.yml HOST:~/inert-site/
    ssh HOST 'cd ~/inert-site && docker compose up -d'

Without a domain the tunnel is a quick tunnel; its address
(`https://<words>.trycloudflare.com`) is in `docker compose logs cloudflared`
and changes when the tunnel restarts. With a domain, create a named tunnel in
Cloudflare and switch the command to
`tunnel --no-autoupdate --config /etc/cloudflared/config.yml run`.
