# rustref.com - Rust documentation redirects
Source code for https://rustref.com - a shorthand for navigating to [Rust](https://rust-lang.org) documentation.

Examples:
```
std.rustref.com -> standard libary documentation
cook.rustref.com -> rust cookbook
ex.rustref.com -> rust by example
...
```

Built with [Rocket](https://rocket.rs) 🚀

## Overview
When a request to `*.rustref.com/**` (that is not `www`), a Cloudflare page rule transforms the request to: `https://rustref.com/redirect/*/**` which then contacts the Rocket server (or uses the cached value in Cloudflare if available).
The Rocket server then sends a 302 response code with the redirect domain to Cloudflare.
Cloudflare will cache this value for 7 days in their proxy, and also set the cache header for the client to expire in 8 days.
This should hopefully make subsequent lookups faster, no matter where you are in the world. 

Redirect information is stored in the `redirects.toml` file in this repository, and the Rocket server converts it into a HashMap for fast lookups. 

The redirects in the HashMap are updated whenever `redirects.toml` in the master branch is changed.

Unfortunately Cloudflare does not offer wildcard proxied CNAME dns records, so the Rocket server makes a new CNAME record for each `short` field in `redirects.toml`.
I think normally people would use a bunch of Page rules with Cloudflare so an origin server isn't needed, but Cloudflare only offers 3 free page rules per domain, and I'm cheap.

This is my first website with an actual server/backend, so if I'm doing something wrong, let me know!

CSS ~~copied~~ inspired by http://neverssl.com

## Hosting/architecture
Browser -> Cloudflare (free tier) -> Google Cloud Platform free tier (US Central) -> [Caddy](https://caddyserver.com) -> Rocket server

## Contributing
Modify `redirects.toml` with a new redirect (in alphabetic order) then make a pull request. 
CI (not setup yet!) will check that the links are valid, and when merged to master a webhook will tell the server to update its redirect HashMap, and clear Cloudflare's cache.

If there is an official site like this, let me know and I can redirect all traffic there.
