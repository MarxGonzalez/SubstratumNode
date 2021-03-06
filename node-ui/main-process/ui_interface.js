// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

const webSocketWrapper = require('./wrappers/websocket_wrapper.js')

module.exports = (() => {
  const DEFAULT_UI_PORT = 5333
  const UI_INTERFACE_URL = `ws://127.0.0.1`
  const UI_PROTOCOL = 'SubstratumNode-UI'
  let webSocket = null
  let getNodeDescriptorCallbackPair = null

  function connect () {
    return new Promise((resolve, reject) => {
      let ws = createSocket(DEFAULT_UI_PORT)
      ws.onopen = () => {
        webSocket = ws
        resolve(true)
      }
      ws.onmessage = (evt) => {
        const data = JSON.parse(evt.data)

        const nodeDescriptor = data['NodeDescriptor']
        if (nodeDescriptor) {
          getNodeDescriptorCallbackPair.resolve(nodeDescriptor)
        }
      }
      ws.onerror = (event) => {
        if (getNodeDescriptorCallbackPair) {
          getNodeDescriptorCallbackPair.reject()
        }
        webSocket = null
        reject(event)
      }
    })
  }

  function isConnected () {
    return !!webSocket
  }

  async function verifyNodeUp (timeoutMillis) {
    return new Promise((resolve) => {
      if (timeoutMillis <= 0) {
        resolve(false)
      } else {
        const finishBy = Date.now() + timeoutMillis
        const onerror = () => {
          setTimeout(async () => {
            const nextTimeout = finishBy - Date.now()
            resolve(await verifyNodeUp(nextTimeout))
          }, 250)
        }

        try {
          const socket = createSocket(DEFAULT_UI_PORT)
          socket.onopen = () => {
            socket.close()
            resolve(true)
          }
          socket.onerror = onerror
        } catch (error) {
          onerror()
        }
      }
    })
  }

  /**
   * tries to connect to the websocket. if it fails other than by timeout it returns true
   * @param timeoutMillis
   * @returns {Promise<boolean>}
   */
  async function verifyNodeDown (timeoutMillis) {
    return new Promise((resolve) => {
      if (timeoutMillis <= 0) {
        resolve(false)
      } else {
        const finishBy = Date.now() + timeoutMillis
        const onerror = () => {
          resolve(true)
        }
        try {
          const socket = createSocket(DEFAULT_UI_PORT)
          socket.onopen = () => {
            socket.close()
            setTimeout(async () => {
              const nextTimeout = finishBy - Date.now()
              resolve(await verifyNodeDown(nextTimeout))
            }, 250)
          }
          socket.onerror = onerror
        } catch (error) {
          onerror()
        }
      }
    })
  }

  function shutdown () {
    webSocket.send('"ShutdownMessage"')
    webSocket.close()
    webSocket = null
  }

  async function getNodeDescriptor () {
    if (getNodeDescriptorCallbackPair) {
      return Promise.reject(Error('CallAlreadyInProgress'))
    }
    return new Promise((resolve, reject) => {
      getNodeDescriptorCallbackPair = {
        resolve: (descriptor) => {
          getNodeDescriptorCallbackPair = null
          resolve(descriptor)
        },
        reject: (e) => {
          reject(e)
        }
      }
      webSocket.send('"GetNodeDescriptor"')
    })
  }

  function createSocket (port) {
    return webSocketWrapper.create(`${UI_INTERFACE_URL}:${port}`, UI_PROTOCOL)
  }

  return {
    DEFAULT_UI_PORT: DEFAULT_UI_PORT,
    UI_INTERFACE_URL: UI_INTERFACE_URL,
    UI_PROTOCOL: UI_PROTOCOL,
    connect: connect,
    isConnected: isConnected,
    verifyNodeUp: verifyNodeUp,
    verifyNodeDown: verifyNodeDown,
    shutdown: shutdown,
    getNodeDescriptor: getNodeDescriptor
  }
})()
