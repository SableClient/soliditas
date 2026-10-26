import { describe, it, expect } from 'vitest';

import worker from '../src/index';

const env = {} as { SERVERNAME: any; HOSTNAME: any; PORT: any };

function embed(query: string) {
	return worker.fetch(new Request(`https://gifs.example/_soliditas/embed/youtube?${query}`), env, {});
}

describe('youtube embed', () => {
	it.each([
		['', ''],
		['&start=0', '&amp;start=0'],
		['&start=90', '&amp;start=90'],
	])('embeds the video with %s', async (query, start) => {
		const response = await embed(`v=MTn_bhTVr2U${query}`);
		const html = await response.text();

		expect(response.status).toBe(200);
		expect(response.headers.get('Content-Type')).toBe('text/html; charset=utf-8');
		expect(html).toContain(`src="https://www.youtube-nocookie.com/embed/MTn_bhTVr2U?autoplay=1${start}"`);
		expect(html).toContain('allow="autoplay; encrypted-media; picture-in-picture; fullscreen" allowfullscreen');
	});

	it('allows custom-scheme parents and sends the service origin to YouTube', async () => {
		const response = await embed('v=MTn_bhTVr2U');

		expect(response.headers.get('Referrer-Policy')).toBe('strict-origin-when-cross-origin');
		expect(response.headers.get('Content-Security-Policy')).toBe("default-src 'none'; style-src 'unsafe-inline'; frame-src https://www.youtube-nocookie.com; base-uri 'none'; form-action 'none'");
		expect(response.headers.has('X-Frame-Options')).toBe(false);
		expect(response.headers.get('Cache-Control')).toBe('public, max-age=86400');
	});

	it('accepts the full video id charset', async () => {
		expect((await embed('v=aB09_-aB09_')).status).toBe(200);
	});

	it.each([
		'', 'v=', 'v=short', 'v=MTn_bhTVr2Ux', 'v=MTn_bhTVr2%2F', 'v=%22%3E%3Cscript%3E',
		'v=MTn_bhTVr2U%0A', 'v=MTn_bhTVr2U&start=', 'v=MTn_bhTVr2U&start=-1',
		'v=MTn_bhTVr2U&start=1.5', 'v=MTn_bhTVr2U&start=1e2', 'v=MTn_bhTVr2U&start=90s',
		'v=MTn_bhTVr2U&start=%201', 'v=MTn_bhTVr2U&start=1%0A',
	])('rejects %s', async (query) => {
		expect((await embed(query)).status).toBe(400);
	});
});
