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

import { matrixEndpointNotImplemented, matrixInvalidParam } from './matrixError';
import { toMatrixID } from './mxcId';
import { proxyMediaCall } from './proxy';
import { returnMatrixServerVers } from './serverversion';
import { MatrixWellKnownServer, SoliditasAddressConvertResponse } from './types';

export default {
	async fetch(request: { url: string | URL; }, env: {
		SERVERNAME: any; HOSTNAME: any; PORT: any; 
}, context: any) {
		const url = new URL(request.url);
		const serverName = env.SERVERNAME || url.hostname;
		const delegate = `${env.HOSTNAME || url.hostname}:${env.PORT || url.port || 443}`;

		if (url.pathname === '/_matrix/federation/v1/version') {
			return new Response(JSON.stringify(returnMatrixServerVers()), {
				headers: { 'Content-Type': 'application/json' },
			});
		} else if (url.pathname.startsWith('/_matrix/federation/v1/media/download/')) {
			const mediaId = url.pathname.replace('/_matrix/federation/v1/media/download/', '');
			return proxyMediaCall(mediaId);
		} else if (url.pathname === '/_soliditas/adressconvert'){
			// helper function mainly for debug reasons as it should be embeded in the client for production use
			const remoteType = url.searchParams.get('remoteType');
			const remoteId = url.searchParams.get('remoteId')
			if (!remoteType || !remoteId ){
				return new Response(JSON.stringify(matrixInvalidParam('invalid parameter')), {
					headers: { 'Content-Type': 'application/json' },
					status: 400,
					statusText: 'invalid parameter',
				})
			}
			const matrixId = toMatrixID(remoteId, `${remoteType}_`)
			const mxcUrl = `mxc://${serverName}/${matrixId}`
			return new Response(JSON.stringify({
				remoteType,
				remoteId,
				mxcUrl,
				mxcId: matrixId
			} satisfies SoliditasAddressConvertResponse))
		} else if (url.pathname === '/.well-known/matrix/server') {
			return new Response(JSON.stringify({ 'm.server': delegate } satisfies MatrixWellKnownServer), {
				headers: { 'Content-Type': 'application/json' },
			});
		} else {
			return new Response(JSON.stringify(matrixEndpointNotImplemented('only implements media endpoints')), {
				headers: { 'Content-Type': 'application/json' },
				status: 404,
        		statusText: 'only implements media endpoints'
			});
		}
	},
};
