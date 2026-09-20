import { describe, it, expect } from 'vitest';

import worker from '../src/index';

const env = {} as { SERVERNAME: any; HOSTNAME: any; PORT: any };

describe('well-known delegation', () => {
	it('delegates to the address the worker was reached on', async () => {
		const response = await worker.fetch(
			{ url: 'https://gifs.example/.well-known/matrix/server' },
			env,
			{}
		);

		expect(await response.json()).toEqual({ 'm.server': 'gifs.example:443' });
		expect(response.headers.get('Content-Type')).toBe('application/json');
	});

	it('keeps the port it was reached on', async () => {
		const response = await worker.fetch(
			{ url: 'https://gifs.example:8443/.well-known/matrix/server' },
			env,
			{}
		);

		expect(await response.json()).toEqual({ 'm.server': 'gifs.example:8443' });
	});

	it('delegates to a pinned hostname and port when one is configured', async () => {
		const response = await worker.fetch({ url: 'https://gifs.example/.well-known/matrix/server' }, {
			...env,
			HOSTNAME: 'proxy.example',
			PORT: 8448,
		}, {});

		expect(await response.json()).toEqual({ 'm.server': 'proxy.example:8448' });
	});
});

describe('address convert', () => {
	it('builds the mxc url on the address the worker was reached on', async () => {
		const response = await worker.fetch(
			{ url: 'https://gifs.example/_soliditas/adressconvert?remoteType=tenor&remoteId=abc' },
			env,
			{}
		);

		expect(await response.json()).toMatchObject({ mxcUrl: 'mxc://gifs.example/tenor_YWJj' });
	});
});
