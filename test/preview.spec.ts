import { describe, it, expect, vi, afterEach } from 'vitest';

import worker from '../src/index';
import { toMatrixID } from '../src/mxcId';
import { proxyMediaCall } from '../src/proxy';

const env = {} as { SERVERNAME: any; HOSTNAME: any; PORT: any };

function preview(target: string) {
	return worker.fetch({ url: `https://gifs.example/_soliditas/preview_url?url=${encodeURIComponent(target)}` }, env, {});
}

function servePage(html: string, contentType = 'text/html; charset=utf-8') {
	return vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(html, { headers: { 'Content-Type': contentType } }));
}

function serveRoutes(routes: Record<string, () => Response>) {
	return vi.spyOn(globalThis, 'fetch').mockImplementation(async (input) => {
		const url = String(input instanceof Request ? input.url : input);
		const route = Object.keys(routes).find((prefix) => url.startsWith(prefix));
		return route ? routes[route]() : new Response(null, { status: 404 });
	});
}

function html(body: string) {
	return new Response(body, { headers: { 'Content-Type': 'text/html' } });
}

function json(body: unknown) {
	return Response.json(body);
}

afterEach(() => {
	vi.restoreAllMocks();
});

describe('preview_url', () => {
	it('answers with the open graph tags and an mxc for the image', async () => {
		servePage(`<html><head>
			<meta property="og:title" content="A post">
			<meta property="og:description" content="Something happened">
			<meta property="og:site_name" content="Example">
			<meta property="og:image" content="/cover.png">
			<meta property="og:image:width" content="640">
			<meta property="og:image:height" content="nope">
		</head></html>`);

		const response = await preview('https://news.example/post');

		expect(response.status).toBe(200);
		expect(response.headers.get('Access-Control-Allow-Origin')).toBe('*');
		expect(await response.json()).toEqual({
			'og:title': 'A post',
			'og:description': 'Something happened',
			'og:site_name': 'Example',
			'og:image': `mxc://gifs.example/${toMatrixID('https://news.example/cover.png', 'og_')}`,
			'og:image:width': 640,
		});
	});

	it('falls back to the title and description tags', async () => {
		servePage('<html><head><title> Plain &amp; page </title><meta name="description" content="No og here"></head></html>');

		expect(await (await preview('https://plain.example/')).json()).toEqual({
			'og:title': 'Plain & page',
			'og:description': 'No og here',
		});
	});

	it('drops an image on a private host', async () => {
		servePage('<meta property="og:title" content="t"><meta property="og:image" content="http://127.0.0.1/x.png">');

		expect(await (await preview('https://news.example/post')).json()).toEqual({ 'og:title': 't' });
	});

	it('refuses urls that are not public http(s)', async () => {
		const fetchSpy = vi.spyOn(globalThis, 'fetch');

		for (const target of ['ftp://files.example/a', 'http://localhost/', 'http://10.0.0.1/', 'http://[::1]/', 'not a url']) {
			expect((await preview(target)).status).toBe(400);
		}
		expect((await worker.fetch({ url: 'https://gifs.example/_soliditas/preview_url' }, env, {})).status).toBe(400);
		expect(fetchSpy).not.toHaveBeenCalled();
	});

	it('answers not found for a page that is not html', async () => {
		servePage('{}', 'application/json');

		expect((await preview('https://api.example/')).status).toBe(404);
	});

	it('answers not found when the page cannot be fetched', async () => {
		vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('down'));

		expect((await preview('https://down.example/')).status).toBe(404);
	});
});

describe('preview_url fallbacks', () => {
	it('fills gaps from the twitter card tags and decodes entities', async () => {
		servePage(`<meta name="twitter:title" content="Tom &amp; Jerry">
			<meta name="twitter:description" content="Cat &#x26; mouse">
			<meta name="twitter:image" content="https://cdn.example/card.png?a=1&amp;b=2">`);

		expect(await (await preview('https://cartoons.example/')).json()).toEqual({
			'og:title': 'Tom & Jerry',
			'og:description': 'Cat & mouse',
			'og:image': `mxc://gifs.example/${toMatrixID('https://cdn.example/card.png?a=1&b=2', 'og_')}`,
		});
	});

	it('prefers og tags over twitter card tags', async () => {
		servePage('<meta name="twitter:title" content="card"><meta property="og:title" content="og">');

		expect(await (await preview('https://site.example/')).json()).toEqual({ 'og:title': 'og' });
	});

	it('fills missing fields from a discovered oembed document', async () => {
		serveRoutes({
			'https://site.example/post': () =>
				html('<meta property="og:title" content="Post"><link rel="alternate" type="application/json+oembed" href="/oembed?id=1">'),
			'https://site.example/oembed?id=1': () =>
				json({ title: 'ignored', provider_name: 'Site', thumbnail_url: 'https://cdn.example/t.jpg', thumbnail_width: 320, thumbnail_height: 180 }),
		});

		expect(await (await preview('https://site.example/post')).json()).toEqual({
			'og:title': 'Post',
			'og:site_name': 'Site',
			'og:image': `mxc://gifs.example/${toMatrixID('https://cdn.example/t.jpg', 'og_')}`,
			'og:image:width': 320,
			'og:image:height': 180,
		});
	});

	it('takes the post title from the reddit oembed over its generic description', async () => {
		serveRoutes({
			'https://www.reddit.com/r/pics/comments/92dd8/': () =>
				html('<meta property="og:title" content="From the pics community on Reddit"><meta property="og:description" content="Explore this post">'),
			'https://www.reddit.com/oembed?format=json&url=https%3A%2F%2Fwww.reddit.com%2Fr%2Fpics%2Fcomments%2F92dd8%2F': () =>
				json({ title: 'test post please ignore', provider_name: 'reddit' }),
		});

		expect(await (await preview('https://www.reddit.com/r/pics/comments/92dd8/')).json()).toEqual({
			'og:title': 'From the pics community on Reddit',
			'og:description': 'test post please ignore',
			'og:site_name': 'reddit',
		});
	});

	it('answers from the oembed alone when the page is blocked', async () => {
		serveRoutes({
			'https://www.tiktok.com/oembed': () => json({ title: 'caption', author_name: 'Scout', provider_name: 'TikTok' }),
		});

		expect(await (await preview('https://www.tiktok.com/@scout/video/1')).json()).toEqual({
			'og:title': 'caption',
			'og:description': 'caption',
			'og:site_name': 'TikTok',
		});
	});
});

describe('preview_url for tweets', () => {
	const author = { name: 'NASA', screen_name: 'NASA', avatar_url: 'https://pbs.twimg.com/avatar.jpg' };

	it('reads the tweet from the fxtwitter api', async () => {
		const fetchSpy = serveRoutes({
			'https://api.fxtwitter.com/status/123': () =>
				json({
					tweet: {
						text: 'Liftoff',
						author,
						media: { photos: [{ url: 'https://pbs.twimg.com/media/a.jpg', width: 3000, height: 2000 }] },
					},
				}),
		});

		expect(await (await preview('https://x.com/NASA/status/123?s=20')).json()).toEqual({
			'og:site_name': 'X',
			'og:title': 'NASA (@NASA)',
			'og:description': 'Liftoff',
			'og:image': `mxc://gifs.example/${toMatrixID('https://pbs.twimg.com/media/a.jpg', 'og_')}`,
			'og:image:width': 3000,
			'og:image:height': 2000,
		});
		expect(fetchSpy).toHaveBeenCalledTimes(1);
	});

	it('uses the mosaic for several photos and the avatar for none', async () => {
		serveRoutes({
			'https://api.fxtwitter.com/status/1': () =>
				json({ tweet: { author, media: { photos: [{ url: 'a' }, { url: 'b' }], mosaic: { formats: { jpeg: 'https://mosaic.example/1' } } } } }),
			'https://api.fxtwitter.com/status/2': () => json({ tweet: { author, text: 'hi' } }),
		});

		expect((await (await preview('https://twitter.com/NASA/status/1')).json()) as object).toMatchObject({
			'og:image': `mxc://gifs.example/${toMatrixID('https://mosaic.example/1', 'og_')}`,
		});
		expect((await (await preview('https://fixupx.com/NASA/status/2')).json()) as object).toMatchObject({
			'og:image': `mxc://gifs.example/${toMatrixID('https://pbs.twimg.com/avatar.jpg', 'og_')}`,
		});
	});

	it('falls back to the page when the api has nothing', async () => {
		serveRoutes({ 'https://x.com/NASA/status/9': () => html('<meta property="og:title" content="from the page">') });

		expect(await (await preview('https://x.com/NASA/status/9')).json()).toEqual({ 'og:title': 'from the page' });
	});
});

describe('og image proxying', () => {
	it('redirects to the image the id carries', async () => {
		const response = await proxyMediaCall(toMatrixID('https://news.example/cover.png', 'og_'));

		expect(response.status).toBe(200);
		expect(await response.text()).toContain('Location: https://news.example/cover.png\r\n');
	});

	it('refuses an image on a private host', async () => {
		const response = await proxyMediaCall(toMatrixID('http://192.168.1.1/cover.png', 'og_'));

		expect(response.status).toBe(400);
	});
});
