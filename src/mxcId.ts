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

/**
 * helper function to convert to base64
 *
 * @param {string} value the string
 * @return {*}  {string} the url-safe-base64 encoded string
 */
function toBase64Url(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = '';

  for (const byte of bytes) {
    binary += String.fromCodePoint(byte);
  }

  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replaceAll(/=+$/g, '');
}

/**
 * helper function to convert from base 64 to a string
 *
 * @param {string} value the base64 encoded url
 * @return {*}  {string} the usable string
 */
function fromBase64Url(value: string): string {
  const normalized = value.replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

/**
 * convert a file name to a mxc safe id
 *
 * @export
 * @param {string} fname the file name
 * @return {*} {string} the mxc id
 */
export function toMatrixID(fname: string, prefix: string): string {
  const base64 = toBase64Url(fname);
  return prefix + base64;
}

/**
 * convert a mxc id to a file name we can actually use
 *
 * @export
 * @param {string} mxcID the matrix mxc id
 * @return {*}  {string} the file name we can actually use and resolve
 */
export function fromMatrixID(mxcID: string): string {
  const separatorIndex = mxcID.indexOf('_');
  if (separatorIndex === -1 || separatorIndex === mxcID.length - 1) {
    throw new Error('Invalid MXC id: missing encoded payload');
  }

  const base64 = mxcID.slice(separatorIndex + 1);
  return fromBase64Url(base64);
}