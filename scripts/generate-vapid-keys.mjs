import { createECDH } from 'node:crypto';

const base64url = (value) => value.toString('base64').replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
const curve = createECDH('prime256v1');
curve.generateKeys();

console.log('FLOWBOARD_WEB_PUSH_VAPID_PUBLIC_KEY=' + base64url(curve.getPublicKey(undefined, 'uncompressed')));
console.log('FLOWBOARD_WEB_PUSH_VAPID_PRIVATE_KEY=' + base64url(curve.getPrivateKey()));
