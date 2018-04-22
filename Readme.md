# rustref.com - Rust documentation redirects
Source code for https://rustref.com

Built with Rocket 🚀

Website design ~~copied~~ _inspired_ by http://neverssl.com with the original CSS author being: Mark Webster https://gist.github.com/markcwebster/9bdf30655cdd5279bad13993ac87c85d

## Overview
When a request to `*.rustref.com/**` (that is not `www`), a Cloudflare page rule transforms the request to: `https://rustref.com/redirect/*/**` which then contacts the Rocket server.
The Rocket server then sends a 302 response code with the redirect domain to Cloudflare.
Cloudflare will cache this value for 7 days in their proxy, and also set the cache header for the client to expire in 8 days.
This should hopefully make subsequent lookups fast, no matter where you are in the world.

Redirect information is stored in the `redirects.toml` file in this repository, and the Rocket server converts that into a HashMap for fast lookups. 
Currently planning on adding ability for the Rocket server to listen to Github webhooks, so whenever the master branch is updated in this repo the server will automatically load the new configuration.

Unfortunately Cloudflare does not offer wildcard proxied CNAME dns records, so the Rocket server just makes a new CNAME record for each `short` field in `redirects.toml`.
I think normally people would use a bunch of Page rules with Cloudflare so an origin server isn't needed, but Cloudflare only offers 3 free page rules per domain, and I'm cheap.

## Contributing
Modify `redirects.toml` with a new redirect (in alphabetic order) then make a pull request. 
CI (not setup yet!) will check that the links are valid, and if approved will generate the website and modify the Netlify redirect rules.

If there is an official site like this, let me know and I can redirect all traffic there.
