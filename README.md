# Soliditas

A matrix media proxy as cloudflare worker.

## Environment variables

- `HOSTNAME`: the hostname, for example `example.org`
- `SERVERNAME`: the servername, for example `example.org`

## Constructing media id

Example in TypeScript, as seen in `mxcId.ts`:

```typescript
function toBase64Url(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = '';

  for (const byte of bytes) {
    binary += String.fromCodePoint(byte);
  }

  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replaceAll(/=+$/g, '');
}

function toMatrixID(fname: string, prefix: string): string {
  const base64 = toBase64Url(fname);
  return prefix + base64;
}
```

You can test it against the `/_soliditas/adressconvert?remoteType=[remoteType]&remoteId=[remoteID]` endpoint.

## Supported proxied upstreams

### Giphy

You need to generate a matrix id, using the algorithm as shown above.

**Example**:

1. The giphy id is `yXPquATCb8kGk`, this has been translated using the `toMatrixID` function, see above, to `giphy_XPquATCb8kGk`.
2. mxc: `mxc://example.org/giphy_XPquATCb8kGk`
3. will return a redirect to `https://i.giphy.com/yXPquATCb8kGk.webp`

### Tenor

You need to generate a matrix id, using the algorithm as shown above.

**Example**:

*To-Do*

### Klipy

You need to generate a matrix id, using the algorithm as shown above.

**Example**:

1. The path on the klipy cdn is `ffd4ac143e6335ac68951b787d3c1902/e8/3a/5LM0jRpL.gif`
2. mxc: `mxc://example.org/klipy_ZmZkNGFjMTQzZTYzMzVhYzY4OTUxYjc4N2QzYzE5MDIvZTgvM2EvNUxNMGpScEwuZ2lm`
3. will return a redirect to `https://static.klipy.com/ii/ffd4ac143e6335ac68951b787d3c1902/e8/3a/5LM0jRpL.gif`

## Link previews

`GET /_soliditas/preview_url?url=[url]` answers in the shape of the Matrix `/preview_url` endpoint. Missing `og:` tags are filled from `twitter:` tags, `<title>`, `<meta name="description">` and oEmbed. Twitter/X links are read from the [FxTwitter API](https://github.com/FxEmbed/FxEmbed), Reddit and TikTok titles from their oEmbed endpoints, Tumblr posts without an image get the [fxtumblr](https://github.com/knuxify/fxtumblr) render, and Misskey-family notes show their first non-sensitive image from the ActivityPub object instead of the author's avatar.

`og:image` is returned as `mxc://[servername]/og_[base64url of the image url]`.

## License

This project is licensed under Apache 2.0, see LICENSE. 
