export function youtubeEmbed(params: URLSearchParams): Response {
	const id = params.get('v');
	const start = params.get('start');
	if (id === null || !/^[A-Za-z0-9_-]{11}$/.test(id) || (start !== null && !/^\d+$/.test(start))) {
		return new Response('Invalid video or start time', { status: 400 });
	}

	const src = `https://www.youtube-nocookie.com/embed/${id}?autoplay=1${start === null ? '' : `&amp;start=${start}`}`;
	return new Response(`<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>YouTube</title>
<style>html,body{margin:0;height:100%}iframe{display:block;width:100%;height:100%;border:0}</style></head>
<body><iframe src="${src}" title="YouTube" allow="autoplay; encrypted-media; picture-in-picture; fullscreen" allowfullscreen></iframe></body></html>`, {
		headers: {
			'Content-Type': 'text/html; charset=utf-8',
			'Referrer-Policy': 'strict-origin-when-cross-origin',
			'Content-Security-Policy': "default-src 'none'; style-src 'unsafe-inline'; frame-src https://www.youtube-nocookie.com; base-uri 'none'; form-action 'none'",
			'Cache-Control': 'public, max-age=86400',
		},
	});
}
