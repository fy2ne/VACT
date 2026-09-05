/**
 * @module transport
 * Public transport exports.
 */

export type { ITransport, TransportEvents, TransportState } from './ITransport.js';
export {
  NamedPipeTransport,
  DEFAULT_PIPE_PATH,
} from './NamedPipeTransport.js';
export type { NamedPipeTransportOptions } from './NamedPipeTransport.js';
export { FrameDecoder, encodeFrame } from './framing.js';
