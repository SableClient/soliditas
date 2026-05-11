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

import { MatrixError } from './types';
import { matrixInvalidParam } from './matrixError';
import { fromMatrixID } from './mxcId';

/**
 * helper function to decode matrix id, and either return the decoded id or a error response
 *
 * @param {string} rawId the id to decoded
 * @return {*}  {(string | MatrixErrorResponse)} decoded id or a error response
 */
function decodeMatrixId(rawId: string): string | MatrixError {
	let id: string;

	try {
		id = fromMatrixID(rawId);
	} catch {
		// parsing the id somehow failed
		return {
			errcode: 'M_INVALID_PARAM',
			error: 'The ID given is not a valid MXC id',
		};
	}

	return id;
}
type MultipartPartLike = {
	headers: HeadersInit;
	body?: BodyInit;
};

function encodeMultipart(boundary: string, parts: MultipartPartLike[]): string {
	const lines: string[] = [];

	for (const part of parts) {
		lines.push(`--${boundary}`);
		for (const [key, value] of Object.entries(part.headers)) {
			lines.push(`${key}: ${value}`);
		}
		lines.push('');
		lines.push(part.body ? String(part.body) : '');
	}

	lines.push(`--${boundary}--`, '');
	return lines.join('\r\n');
}

function buildMultipartRedirect(targetLocation: string): Response {
	const boundary = `soliditas${Date.now().toString(16)}${Math.random().toString(16).slice(2)}`;

	const body = encodeMultipart(boundary, [
		{
			headers: { 'Content-Type': 'application/json' },
			body: '{}',
		},
		{
			headers: {
				'Content-Type': 'application/octet-stream',
				'Location': targetLocation,
			},
		},
	]);

	return new Response(body, {
		headers: {
			'Content-Type': `multipart/mixed; boundary=${boundary}`,
			'Content-Length': `${body.length}`,
		},
		status: 200,
	});
}

/**
 * proxy a giphy call
 * example id: yXPquATCb8kGk
 *
 * @param {string} id the id of the giphy media
 * @return {*}  {Response} the multipart response to return to the requesting homeserver
 */
function proxyGiphy(id: string): Response {
	return buildMultipartRedirect(`https://i.giphy.com/${id}.webp`);
}

function proxyTenor(id: string): Response {
	return buildMultipartRedirect(`https://media.tenor.com/${id}/tenor.gif`);
}

/**
 * proxy a klipy.com gif
 * example for the cdn address format: https://static.klipy.com/ii/ffd4ac143e6335ac68951b787d3c1902/e8/3a/5LM0jRpL.gif
 * @param {string} path the path on the cdn (for example `ffd4ac143e6335ac68951b787d3c1902/e8/3a/5LM0jRpL.gif`)
 * @return {*}  {Response} the multipart response to return to the requesting homeserver
 */
function proxyKlipy(path: string): Response {
	return buildMultipartRedirect(`https://static.klipy.com/ii/${path}`)
}

export async function proxyMediaCall(rawId: string): Promise<Response> {
	const decodedId = decodeMatrixId(rawId);
	if (typeof decodedId !== 'string') {
		return new Response(JSON.stringify(decodedId), { status: 400, statusText: 'error decoding media id' });
	}
	const isGiphy: boolean = rawId.startsWith('giphy_') || decodedId.startsWith('giphy_');
	const isTenor: boolean = rawId.startsWith('tenor_') || decodedId.startsWith('tenor_');
	const isKlipy: boolean = rawId.startsWith('klipy_') || decodedId.startsWith('klipy_');
	if (isTenor) {
		return proxyTenor(decodedId);
	} else if (isGiphy) {
		return proxyGiphy(decodedId);
	} else if (isKlipy) {
		return proxyKlipy(decodedId);
	}
	return new Response(JSON.stringify(matrixInvalidParam("the identifier of the remote didn't match any supported remote identifier")), {
		status: 400,
		statusText: "the identifier of the remote didn't match any supported remote identifier",
	});
}
