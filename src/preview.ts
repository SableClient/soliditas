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

const USER_AGENT = 'Soliditas (bot; +https://github.com/SableClient/soliditas) facebookexternalhit/1.1';

const CORS_HEADERS = {
	'Content-Type': 'application/json',
	'Access-Control-Allow-Origin': '*',
};

const TWITTER_HOSTS = /^(?:www\.|mobile\.)?(?:twitter|x|fxtwitter|fixupx|vxtwitter|fixvx)\.com$/;
const TWITTER_STATUS = /^\/(?:[^/]+|i\/web)\/status(?:es)?\/(\d+)/;

const TUMBLR_BLOG_POST = /^([a-z\d-]+)\.tumblr\.com$/;
const TUMBLR_DASHBOARD_POST = /^\/([a-z\d-]+)\/(\d+)/;

const OEMBED_ENDPOINTS: [RegExp, string][] = [
	[/(?:^|\.)reddit\.com$/, 'https://www.reddit.com/oembed'],
	[/(?:^|\.)tiktok\.com$/, 'https://www.tiktok.com/oembed'],
];

const NAMED_ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: '\u00a0' };

const MAX_IMAGES = 10;

const THEME_COLOR = /^#(?:[\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/i;

const MAX_DIRECT_VIDEO_BYTES = 100 * 1024 * 1024;

const VIDEO_EXTENSION = /\.(?:mp4|webm|mov|m4v|ogv)$/i;

type Tags = Record<string, string>;

type Image = { url: string; width?: number; height?: number };

type Video = { url: string; type: string; width?: number; height?: number };

type Preview = { tags: Tags; images: Image[]; cover?: Image; video?: Video };

type OEmbed = {
	title?: string;
	author_name?: string;
	author_url?: string;
	provider_name?: string;
	thumbnail_url?: string;
	thumbnail_width?: number;
	thumbnail_height?: number;
};

type ActivityNote = {
	sensitive?: boolean;
	attachment?: { mediaType?: string; url?: string; width?: number; height?: number }[];
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

function toSize(value: string | number | undefined): number | undefined {
	const size = typeof value === 'number' ? value : Number.parseInt(value ?? '', 10);
	return Number.isFinite(size) && size > 0 ? size : undefined;
}

function toImage(value: string | undefined, base?: string | URL, width?: string | number, height?: string | number): Image | null {
	const url = value ? parsePublicUrl(value, base) : null;
	if (!url || url.pathname === '/') return null;
	return { url: url.href, width: toSize(width), height: toSize(height) };
}

function toVideo(value: string | undefined, base: string | URL, type?: string, width?: string | number, height?: string | number): Video | null {
	const url = value ? parsePublicUrl(value, base) : null;
	if (!url || url.pathname === '/') return null;
	const mime = type?.split(';')[0].trim().toLowerCase();
	if (mime ? !mime.startsWith('video/') : !VIDEO_EXTENSION.test(url.pathname)) return null;
	return { url: url.href, type: mime ?? 'video/mp4', width: toSize(width), height: toSize(height) };
}

function imageKey(image: Image): string {
	const url = new URL(image.url);
	if (!url.hostname.endsWith('.media.tumblr.com')) return url.href;
	return url.pathname.split('/').slice(1, 3).join('/');
}

function addImages(images: Image[], candidates: (Image | null)[]): void {
	for (const image of candidates) {
		if (image && images.length < MAX_IMAGES && !images.some((known) => imageKey(known) === imageKey(image))) images.push(image);
	}
}

async function readPage(page: Response, base: string | URL): Promise<{ preview: Preview; oembed?: string; misskeyNote: boolean }> {
	const tags: Tags = {};
	const fallback: Tags = {};
	const found: { url: string; width?: string; height?: string }[] = [];
	let title = '';
	let oembed: string | undefined;
	let misskeyNote = false;
	const clip: { url?: string; secure?: string; type?: string; width?: string; height?: string } = {};

	await new HTMLRewriter()
		.on('meta', {
			element(element) {
				const key = element.getAttribute('property') ?? element.getAttribute('name');
				const raw = element.getAttribute('content');
				const content = raw && decodeEntities(raw).trim();
				if (!key || !content) return;
				if (key === 'misskey:note-id') misskeyNote = true;
				if (key === 'og:video' || key === 'og:video:url') clip.url ??= content;
				else if (key === 'og:video:secure_url') clip.secure ??= content;
				else if (key === 'og:video:type') clip.type ??= content;
				else if (key === 'og:video:width') clip.width ??= content;
				else if (key === 'og:video:height') clip.height ??= content;
				else if (key === 'og:image' || key === 'og:image:url') found.push({ url: content });
				else if (key === 'og:image:width' || key === 'og:image:height') {
					const last = found.at(-1);
					const side = key === 'og:image:width' ? 'width' : 'height';
					if (last && !last[side]) last[side] = content;
				} else if (key.startsWith('og:image')) return;
				else if (key.startsWith('og:')) setTag(tags, key, content);
				else if (key.startsWith('twitter:') || key === 'description' || key === 'theme-color') setTag(fallback, key, content);
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

	const color = fallback['theme-color'];
	if (color && THEME_COLOR.test(color)) tags['com.sable.theme_color'] = color;
	const card = fallback['twitter:card'];
	if (card === 'summary' || card === 'summary_large_image') tags['com.sable.card'] = card;

	const images: Image[] = [];
	addImages(images, found.map((image) => toImage(image.url, base, image.width, image.height)));
	if (!images.length) addImages(images, [toImage(fallback['twitter:image'] ?? fallback['twitter:image:src'], base)]);
	const video = toVideo(clip.secure ?? clip.url, base, clip.type, clip.width, clip.height);
	return { preview: { tags, images, video: video ?? undefined }, oembed, misskeyNote };
}

async function noteImages(url: URL): Promise<Image[]> {
	const response = await fetchPublic(url, 'application/activity+json');
	const note = response ? ((await response.json().catch(() => null)) as ActivityNote | null) : null;
	if (!note || note.sensitive) return [];
	const images: Image[] = [];
	addImages(
		images,
		(note.attachment ?? []).filter((file) => file.mediaType?.startsWith('image/')).map((file) => toImage(file.url, url, file.width, file.height)),
	);
	return images;
}

function mergeOEmbed(preview: Preview, oembed: OEmbed, authoritative: boolean): void {
	const { tags } = preview;
	if (authoritative && oembed.title) tags['og:description'] = oembed.title;
	setTag(tags, 'og:title', oembed.title ?? oembed.author_name);
	setTag(tags, 'com.sable.author_name', oembed.author_name);
	setTag(tags, 'og:site_name', oembed.provider_name);
	if (!preview.images.length) addImages(preview.images, [toImage(oembed.thumbnail_url, undefined, oembed.thumbnail_width, oembed.thumbnail_height)]);
}

async function previewTweet(id: string): Promise<Preview | null> {
	const body = await fetchJson<{ tweet?: FxTweet }>(new URL(`https://api.fxtwitter.com/status/${id}`));
	const tweet = body?.tweet;
	if (!tweet) return null;

	const tags: Tags = { 'og:site_name': 'X' };
	const { name, screen_name: handle, avatar_url: avatar } = tweet.author ?? {};
	setTag(tags, 'og:title', name && handle ? `${name} (@${handle})` : (name ?? handle));
	setTag(tags, 'og:description', tweet.text);

	const images: Image[] = [];
	const media = [...(tweet.media?.photos ?? []), ...(tweet.media?.videos ?? [])];
	addImages(images, media.map((item) => toImage(item.thumbnail_url ?? item.url, undefined, item.width, item.height)));
	if (!images.length) addImages(images, [toImage(avatar)]);

	const mosaic = images.length > 1 ? toImage(tweet.media?.mosaic?.formats?.jpeg) : null;
	return { tags, images, cover: mosaic ?? undefined };
}

function imageType(page: Response): string | null {
	const type = /^\s*(image\/[\w.+-]+)/i.exec(page.headers.get('Content-Type') ?? '')?.[1].toLowerCase();
	return type && type !== 'image/svg+xml' ? type : null;
}

function videoType(page: Response): string | null {
	return /^\s*(video\/[\w.+-]+)/i.exec(page.headers.get('Content-Type') ?? '')?.[1].toLowerCase() ?? null;
}

function videoSize(page: Response): number {
	return toSize(page.headers.get('Content-Length') ?? undefined) ?? 0;
}

async function previewPage(url: URL): Promise<Preview | null> {
	const endpoint = OEMBED_ENDPOINTS.find(([host]) => host.test(url.hostname))?.[1];
	const known = endpoint ? fetchJson<OEmbed>(new URL(`${endpoint}?format=json&url=${encodeURIComponent(url.href)}`)) : null;

	const page = await fetchPublic(url, 'text/html');
	const base = page?.url || url;
	const clipType = page && videoType(page);
	if (page && clipType) {
		await page.body?.cancel();
		const video = toVideo(base.toString(), base, clipType);
		return video && videoSize(page) <= MAX_DIRECT_VIDEO_BYTES ? { tags: {}, images: [], video } : null;
	}
	const type = page && imageType(page);
	if (page && type) {
		await page.body?.cancel();
		const image = toImage(base.toString());
		return image ? { tags: { 'og:image:type': type }, images: [image] } : null;
	}
	const html = page?.headers.get('Content-Type')?.includes('text/html') ? await readPage(page, base) : null;
	const knownOEmbed = await known;
	if (!html && !knownOEmbed) return null;

	const preview = html?.preview ?? { tags: {}, images: [] };
	const { tags } = preview;
	if (knownOEmbed) mergeOEmbed(preview, knownOEmbed, true);
	else if (html?.oembed && !(tags['og:title'] && tags['og:description'] && preview.images.length)) {
		const discovered = parsePublicUrl(html.oembed, base);
		const found = discovered ? await fetchJson<OEmbed>(discovered) : null;
		if (found) mergeOEmbed(preview, found, false);
	}
	if (html?.misskeyNote) {
		const attachments = await noteImages(url);
		if (attachments.length) preview.images = attachments;
	}
	return preview;
}

function tumblrPost(url: URL): string | null {
	const blog = TUMBLR_BLOG_POST.exec(url.hostname)?.[1];
	if (blog && blog !== 'www') {
		const id = /^\/post\/(\d+)/.exec(url.pathname)?.[1];
		return id ? `${blog}/${id}` : null;
	}
	if (!/^(?:www\.)?tumblr\.com$/.test(url.hostname)) return null;
	const [, dashboardBlog, id] = TUMBLR_DASHBOARD_POST.exec(url.pathname) ?? [];
	return id ? `${dashboardBlog}/${id}` : null;
}

async function previewTumblr(url: URL, post: string): Promise<Preview | null> {
	const [preview, fxtumblr] = await Promise.all([previewPage(url), previewPage(new URL(`https://tpmblr.com/${post}`))]);
	if (!preview || preview.images.length || !fxtumblr?.images.length) return preview ?? fxtumblr;
	preview.images = fxtumblr.images;
	return preview;
}

function toMatrixPreview({ tags, images, cover, video }: Preview, serverName: string): Record<string, unknown> {
	const toMxc = (image: Image) => `mxc://${serverName}/${toMatrixID(image.url, 'og_')}`;
	const preview: Record<string, unknown> = { ...tags };

	const main = cover ?? images[0];
	if (main) {
		preview['og:image'] = toMxc(main);
		if (main.width) preview['og:image:width'] = main.width;
		if (main.height) preview['og:image:height'] = main.height;
	}
	if (video) {
		preview['og:video'] = toMxc({ url: video.url });
		preview['og:video:type'] = video.type;
		if (video.width) preview['og:video:width'] = video.width;
		if (video.height) preview['og:video:height'] = video.height;
	}
	if (images.length > 1) {
		preview['com.sable.images'] = images.map((image) => ({ url: toMxc(image), width: image.width, height: image.height }));
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
	const tumblr = tumblrPost(url);
	const preview = (tweet ? await previewTweet(tweet) : null) ?? (tumblr ? await previewTumblr(url, tumblr) : await previewPage(url));
	if (!preview) {
		return new Response(JSON.stringify(matrixRessourceNotFound('no html preview for this url')), {
			headers: CORS_HEADERS,
			status: 404,
		});
	}

	return new Response(JSON.stringify(toMatrixPreview(preview, serverName)), {
		headers: { ...CORS_HEADERS, 'Cache-Control': 'public, max-age=86400' },
	});
}
