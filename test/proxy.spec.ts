import { describe, it, expect, vi, beforeEach } from 'vitest';

import { proxyMediaCall } from '../src/proxy';
import * as mxcId from '../src/mxcId';

describe('proxyMediaCall', () => {
	it('returns 400 when matrix id decoding fails', async () => {
		const response = await proxyMediaCall('invalid');

		expect(response.status).toBe(400);

		const json = await response.json();

		expect(json).toEqual({
			errcode: 'M_INVALID_PARAM',
			error: 'The ID given is not a valid MXC id',
		});
	});

	describe('giphy proxying', () => {
		it('creates redirect without the giphy_ prefix', async () => {
			const response = await proxyMediaCall('giphy_eVhQcXVBVENiOGtHaw');

			expect(response.status).toBe(200);

			const body = await response.text();

			// to ensure it has the Location part needed for forwarding
			expect(body).toContain('Location:');

			expect(body).toContain('https://i.giphy.com/yXPquATCb8kGk.webp');

			expect(body).not.toContain('https://i.giphy.com/giphy_yXPquATCb8kGk.webp');
		});
		it('creates a valid multipart redirect response for giphy', async () => {
			const response = await proxyMediaCall('giphy_eVhQcXVBVENiOGtHaw');
			expect(response.status).toBe(200);

			const contentType = response.headers.get('Content-Type');
			expect(contentType).toMatch(/^multipart\/mixed; boundary=soliditas/);

			const boundary = contentType!.match(/boundary=(.+)$/)?.[1];
			expect(boundary).toBeDefined();

			const body = await response.text();
			const expected =
				`--${boundary}\r\n` +
				`Content-Type: application/json\r\n` +
				`\r\n` +
				`{}\r\n` +
				`--${boundary}\r\n` +
				`Content-Type: application/octet-stream\r\n` +
				`Location: https://i.giphy.com/yXPquATCb8kGk.webp\r\n` +
				`\r\n` +
				`\r\n` +
				`--${boundary}--\r\n`;
			expect(body).toBe(expected);
			expect(response.headers.get('Content-Length')).toBe(String(Buffer.byteLength(expected)));
		});
	});

	describe('tenor proxying', () => {
		it('creates redirect without the tenor_ prefix', async () => {
			const response = await proxyMediaCall('tenor_YWJjMTIz');

			expect(response.status).toBe(200);

			const body = await response.text();

			// to ensure it has the Location part needed for forwarding
			expect(body).toContain('Location:');

			expect(body).toContain('https://media.tenor.com/abc123/tenor.gif');

			expect(body).not.toContain('https://media.tenor.com/tenor_abc123/tenor.gif');
		});
		it('creates a valid multipart redirect response for tenor', async () => {
			const response = await proxyMediaCall('tenor_YWJjMTIz');
			expect(response.status).toBe(200);

			const contentType = response.headers.get('Content-Type');
			expect(contentType).toMatch(/^multipart\/mixed; boundary=soliditas/);

			const boundary = contentType!.match(/boundary=(.+)$/)?.[1];
			expect(boundary).toBeDefined();

			const body = await response.text();
			const expected =
				`--${boundary}\r\n` +
				`Content-Type: application/json\r\n` +
				`\r\n` +
				`{}\r\n` +
				`--${boundary}\r\n` +
				`Content-Type: application/octet-stream\r\n` +
				`Location: https://media.tenor.com/abc123/tenor.gif\r\n` +
				`\r\n` +
				`\r\n` +
				`--${boundary}--\r\n`;
			expect(body).toBe(expected);

			expect(response.headers.get('Content-Length')).toBe(String(Buffer.byteLength(expected)));
		});
	});

	describe('klipy proxying', () => {
		it('creates redirect without the klipy_ prefix', async () => {
			const klipyPath = 'ffd4ac143e6335ac68951b787d3c1902/e8/3a/5LM0jRpL.gif';

			const response = await proxyMediaCall(mxcId.toMatrixID(klipyPath, 'klipy_'));

			expect(response.status).toBe(200);

			const body = await response.text();

			// to ensure it has the Location part needed for forwarding
			expect(body).toContain('Location:');

			expect(body).toContain(`https://static.klipy.com/ii/${klipyPath}`);

			expect(body).not.toContain(`https://static.klipy.com/ii/klipy_${klipyPath}`);
		});
        it('creates a valid multipart redirect response for klipy', async () => {
            const klipyPath = 'ffd4ac143e6335ac68951b787d3c1902/e8/3a/5LM0jRpL.gif';
			const response = await proxyMediaCall(mxcId.toMatrixID(klipyPath, 'klipy_'));
			expect(response.status).toBe(200);

			const contentType = response.headers.get('Content-Type');
			expect(contentType).toMatch(/^multipart\/mixed; boundary=soliditas/);

			const boundary = contentType!.match(/boundary=(.+)$/)?.[1];
			expect(boundary).toBeDefined();

			const body = await response.text();
			const expected =
				`--${boundary}\r\n` +
				`Content-Type: application/json\r\n` +
				`\r\n` +
				`{}\r\n` +
				`--${boundary}\r\n` +
				`Content-Type: application/octet-stream\r\n` +
				`Location: https://static.klipy.com/ii/${klipyPath}\r\n` +
				`\r\n` +
				`\r\n` +
				`--${boundary}--\r\n`;
			expect(body).toBe(expected);

			expect(response.headers.get('Content-Length')).toBe(String(Buffer.byteLength(expected)));
		});
	});

	it('returns 400 for unsupported providers', async () => {
		const response = await proxyMediaCall('unsupported_123');

		expect(response.status).toBe(400);

		const body: unknown = await response.json();

		// @ts-ignore
		expect(body.errcode).toBe('M_INVALID_PARAM');
	});
});
