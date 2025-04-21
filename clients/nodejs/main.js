import { createLibp2p } from 'libp2p'
import { tcp } from '@libp2p/tcp'
import { noise } from '@chainsafe/libp2p-noise'
import { yamux } from '@chainsafe/libp2p-yamux'
import { fetch } from '@libp2p/fetch'
import { identify } from '@libp2p/identify'
import { multiaddr } from '@multiformats/multiaddr'
import { pipe } from 'it-pipe'
import all from 'it-all'
import { encode, decode } from 'cbor-x'
import { toString as uint8ArrayToString } from 'uint8arrays/to-string'
import { fromString as uint8ArrayFromString } from 'uint8arrays/from-string'

async function main() {
  const node = await createLibp2p({
    transports: [tcp()],
    streamMuxers: [yamux()],
    connectionEncrypters: [noise()],
    services: {
      // identify: identify(),
      fetch: fetch()
    }
  })

  await node.start()
  console.log('Node started with PeerId:', node.peerId.toString())

  const rustPeerId = '12D3KooWG15p7kdp1jA7G4ZMfBu9n9L9JAxL5Y8LwcTEtfMXEkd3'
  const rustNodeAddr = multiaddr(`/ip4/127.0.0.1/tcp/55784/p2p/${rustPeerId}`)

  const connection = await node.dial(rustNodeAddr)
  
  console.log('Connected to Rust node:', rustNodeAddr.toString())

  const stream = await node.dialProtocol(connection.remotePeer, '/mpc/1.0.0')

  // const request = { Ping: [] }
  const custom = {
    2:
    {
        id: "abc",
        data: Buffer.from([1, 2, 3])  // CBOR supports binary blobs via Buffer
    }
  };
const ping = {0: []};
  // const request = {2: { 'id': '#123', 'data': uint8ArrayFromString('hi from the client') }};

  const cborRequest = encode(custom)
  console.log(cborRequest)

  // const response = await pipe(
  //   [cborRequest],
  //   await node.dialProtocol(connection.remotePeer, '/mpc/1.0.0'),
  //   async function (source) {
  //     const chunks = []
  //     for await (const chunk of source) {
  //       chunks.push(chunk)
  //     }
  //     return new Uint8Array(chunks.reduce((acc, val) => [...acc, ...val], []))
  //   }
  // )

  // const decoded = decode(response)
  // console.log('Received CBOR-decoded response:', decoded)

  const response = await pipe(
    [cborRequest],       // CBOR-encoded request
    stream,
    async function (source) {
      const chunks = await all(source)
      // const chunks = []
      // for await (const chunk of source) {
      //   chunks.push(chunk)
      // }
      console.log('reading the response..:', chunks)
      return new Uint8Array(chunks.reduce((acc, cur) => [...acc, ...cur], []))
      // return new Uint8Array(chunks.flatMap(chunk => [...chunk]))
    }
  )
  console.log('Raw CBOR response bytes:', response)
  // console.log('As UTF-8:', new TextDecoder().decode(response))
  // const decoded = decode(response)
  // console.log('Received CBOR-decoded response:', decoded)
  
  // const requestPayload = uint8ArrayFromString('Hello from Node.js!')

  // const response = await pipe(
  //   [requestPayload],          // Source: the request body
  //   stream,                    // Duplex stream (source + sink)
  //   async function (source) {  // Read response
  //     const chunks = []
  //     for await (const chunk of source) {
  //       chunks.push(chunk)
  //     }
  //     return new Uint8Array(chunks.reduce((acc, val) => [...acc, ...val], []))
  //   }
  // )
  // console.log('Received response:', uint8ArrayToString(response))

  await node.stop()
}

main().catch(console.error)

