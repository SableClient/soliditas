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
import { initSync, parsePublicUrl as parsePublic, scrapePreview } from './wasm/soliditas_wasm.js';
import wasm from './wasm/soliditas_wasm_bg.wasm';

initSync({ module: wasm });

const CORS_HEADERS = {
	'Content-Type': 'application/json',
	'Access-Control-Allow-Origin': '*',
};

export function parsePublicUrl(value: string): URL | null {
	const url = parsePublic(value);
	return url === undefined ? null : new URL(url);
}

export async function previewUrl(target: string | null, serverName: string): Promise<Response> {
	const url = target ? parsePublicUrl(target) : null;
	if (!url) {
		return new Response(JSON.stringify(matrixInvalidParam('url must be a public http(s) url')), {
			headers: CORS_HEADERS,
			status: 400,
		});
	}

	const preview = await scrapePreview(url.href, (media: string) => `mxc://${serverName}/${toMatrixID(media, 'og_')}`);
	if (preview === undefined) {
		return new Response(JSON.stringify(matrixRessourceNotFound('no html preview for this url')), {
			headers: CORS_HEADERS,
			status: 404,
		});
	}

	return new Response(preview, {
		headers: { ...CORS_HEADERS, 'Cache-Control': 'public, max-age=86400' },
	});
}
