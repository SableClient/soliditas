/*
   Apache License 2.0

   Copyright 2026 Rye

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
*/

import { matrixInvalidParam, matrixRessourceNotFound } from './matrixError';
import { toMatrixID } from './mxcId';

const USER_AGENT = 'Mozilla/5.0 (compatible; SoliditasBot/1.0; +https://github.com/SableClient/soliditas) facebookexternalhit/1.1';

const CORS_HEADERS = {
	'Content-Type': 'application/json',
	'Access-Control-Allow-Origin': '*',
};

const TWITTER_HOSTS = /^(?:www\.|mobile\.)?(?:twitter|x|fxtwitter|fixupx|vxtwitter|fixvx)\.com$/;
const TWITTER_STATUS = /^\/(?:[^/]+|i\/web)\/status(?:es)?\/(\d+)/;

const OEMBED_ENDPOINTS: [RegExp, string][] = [
	[/(?:^|\.)reddit\.com$/, 'https://www.reddit.com/oembed'],
	[/(?:^|\.)tiktok\.com$/, 'https://www.tiktok.com/oembed'],
];

const NAMED_ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: '\u00a0' };

type Tags = Record<string, string>;

type OEmbed = {
	title?: string;
	author_name?: string;
	provider_name?: string;
	thumbnail_url?: string;
	thumbnail_width?: number;
	thumbnail_height?: number;
};

type FxMedia = { url?: string; thumbnail_url?: string; width?: number; height?: number };

type FxTweet = {
	text?: string;
	author?: { name?: string; screen_name?: string; avatar_url?: string };
	media?: { photos?: FxMedia[]; videos?: FxMedia[]; mosaic?: { formats?: { jpeg?: string } } };
};

export function parsePublicUrl(value: string, base?: string | URL): URL | null {
	let url: URL;
	try {
		url = new URL(value, base);
	} catch {
		return null;
	}
	if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
	const host = url.hostname.toLowerCase();
	if (!host.includes('.') || host.includes(':') || host.startsWith('[')) return null;
	if (/^[\d.]+$/.test(host)) return null;
	if (host === 'localhost' || /\.(localhost|local|internal|home\.arpa)$/.test(host)) return null;
	return url;
}

function decodeEntities(value: string): string {
	return value.replace(/&(?:#x([\da-f]+)|#(\d+)|([a-z]+));/gi, (entity, hex, decimal, name) => {
		const code = hex ? Number.parseInt(hex, 16) : decimal ? Number.parseInt(decimal, 10) : undefined;
		if (code !== undefined) return code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : entity;
		return NAMED_ENTITIES[name.toLowerCase()] ?? entity;
	});
}

function fetchPublic(url: URL, accept: string): Promise<Response | null> {
	return fetch(url, { headers: { 'User-Agent': USER_AGENT, Accept: accept }, redirect: 'follow' })
		.then((response) => (response.ok ? response : null))
		.catch(() => null);
}

async function fetchJson<T>(url: URL): Promise<T | null> {
	const response = await fetchPublic(url, 'application/json');
	return response ? ((await response.json().catch(() => null)) as T | null) : null;
}

function setTag(tags: Tags, key: string, value: string | number | undefined): void {
	if (value === undefined || value === '' || tags[key]) return;
	tags[key] = String(value);
}

async function readPage(page: Response): Promise<{ tags: Tags; oembed?: string }> {
	const tags: Tags = {};
	const fallback: Tags = {};
	let title = '';
	let oembed: string | undefined;

	await new HTMLRewriter()
		.on('meta', {
			element(element) {
				const key = element.getAttribute('property') ?? element.getAttribute('name');
				const raw = element.getAttribute('content');
				const content = raw && decodeEntities(raw).trim();
				if (!key || !content) return;
				if (key.startsWith('og:')) setTag(tags, key, content);
				else if (key.startsWith('twitter:') || key === 'description') setTag(fallback, key, content);
			},
		})
		.on('link[rel="alternate"][type="application/json+oembed"]', {
			element(element) {
				const href = element.getAttribute('href');
				oembed ??= href ? decodeEntities(href) : undefined;
			},
		})
		.on('title', {
			text(text) {
				title += text.text;
			},
		})
		.transform(page)
		.arrayBuffer();

	setTag(tags, 'og:title', fallback['twitter:title'] ?? decodeEntities(title).trim());
	setTag(tags, 'og:description', fallback['twitter:description'] ?? fallback['description']);
	setTag(tags, 'og:image', fallback['twitter:image'] ?? fallback['twitter:image:src']);
	return { tags, oembed };
}

function mergeOEmbed(tags: Tags, oembed: OEmbed, authoritative: boolean): void {
	if (authoritative && oembed.title) tags['og:description'] = oembed.title;
	setTag(tags, 'og:title', oembed.title ?? oembed.author_name);
	setTag(tags, 'og:site_name', oembed.provider_name);
	if (!tags['og:image'] && oembed.thumbnail_url) {
		tags['og:image'] = oembed.thumbnail_url;
		setTag(tags, 'og:image:width', oembed.thumbnail_width);
		setTag(tags, 'og:image:height', oembed.thumbnail_height);
	}
}

async function previewTweet(id: string): Promise<Tags | null> {
	const body = await fetchJson<{ tweet?: FxTweet }>(new URL(`https://api.fxtwitter.com/status/${id}`));
	const tweet = body?.tweet;
	if (!tweet) return null;

	const tags: Tags = { 'og:site_name': 'X' };
	const { name, screen_name: handle, avatar_url: avatar } = tweet.author ?? {};
	setTag(tags, 'og:title', name && handle ? `${name} (@${handle})` : (name ?? handle));
	setTag(tags, 'og:description', tweet.text);

	const photos = tweet.media?.photos ?? [];
	const mosaic = photos.length > 1 ? tweet.media?.mosaic?.formats?.jpeg : undefined;
	const media = photos[0] ?? tweet.media?.videos?.[0];
	if (mosaic) {
		tags['og:image'] = mosaic;
	} else if (media) {
		setTag(tags, 'og:image', media.thumbnail_url ?? media.url);
		setTag(tags, 'og:image:width', media.width);
		setTag(tags, 'og:image:height', media.height);
	} else {
		setTag(tags, 'og:image', avatar);
	}
	return tags;
}

async function previewPage(url: URL): Promise<Tags | null> {
	const endpoint = OEMBED_ENDPOINTS.find(([host]) => host.test(url.hostname))?.[1];
	const known = endpoint ? fetchJson<OEmbed>(new URL(`${endpoint}?format=json&url=${encodeURIComponent(url.href)}`)) : null;

	const page = await fetchPublic(url, 'text/html');
	const html = page?.headers.get('Content-Type')?.includes('text/html') ? await readPage(page) : null;
	const knownOEmbed = await known;
	if (!html && !knownOEmbed) return null;

	const tags = html?.tags ?? {};
	const base = page?.url || url;
	if (knownOEmbed) mergeOEmbed(tags, knownOEmbed, true);
	else if (html?.oembed && !(tags['og:title'] && tags['og:description'] && tags['og:image'])) {
		const discovered = parsePublicUrl(html.oembed, base);
		const found = discovered ? await fetchJson<OEmbed>(discovered) : null;
		if (found) mergeOEmbed(tags, found, false);
	}

	if (tags['og:image']) tags['og:image'] = parsePublicUrl(tags['og:image'], base)?.href ?? '';
	return tags;
}

function toMatrixPreview(tags: Tags, serverName: string): Record<string, string | number> {
	const preview: Record<string, string | number> = { ...tags };

	delete preview['og:image'];
	const image = tags['og:image'] ? parsePublicUrl(tags['og:image']) : null;
	if (image) preview['og:image'] = `mxc://${serverName}/${toMatrixID(image.href, 'og_')}`;

	for (const key of ['og:image:width', 'og:image:height']) {
		const size = Number.parseInt(tags[key] ?? '', 10);
		if (image && Number.isFinite(size) && size > 0) preview[key] = size;
		else delete preview[key];
	}
	return preview;
}

export async function previewUrl(target: string | null, serverName: string): Promise<Response> {
	const url = target ? parsePublicUrl(target) : null;
	if (!url) {
		return new Response(JSON.stringify(matrixInvalidParam('url must be a public http(s) url')), {
			headers: CORS_HEADERS,
			status: 400,
		});
	}

	const tweet = TWITTER_HOSTS.test(url.hostname) ? TWITTER_STATUS.exec(url.pathname)?.[1] : undefined;
	const tags = (tweet ? await previewTweet(tweet) : null) ?? (await previewPage(url));
	if (!tags) {
		return new Response(JSON.stringify(matrixRessourceNotFound('no html preview for this url')), {
			headers: CORS_HEADERS,
			status: 404,
		});
	}

	return new Response(JSON.stringify(toMatrixPreview(tags, serverName)), {
		headers: { ...CORS_HEADERS, 'Cache-Control': 'public, max-age=86400' },
	});
}
