// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

/* global describe beforeEach it afterEach expect */

const td = require('testdouble')

describe('main', () => {
  let mockApp, mockDialog, mockEvent, mockHttp, mockIpcMain, mockMenu, mainWindowOnClose, MockNodeActuator, appOnReady, ipcMainOnIpLookup, ipcMainOnChangeNodeState, process

  beforeEach(() => {
    mockEvent = td.object(['preventDefault'])
    mockApp = td.object(['getName', 'on', 'quit'])
    mockDialog = td.object(['showErrorBox'])
    mockIpcMain = td.object(['on'])
    MockNodeActuator = td.constructor(['shutdown', 'off', 'serving', 'consuming'])
    mockMenu = td.object(['setApplicationMenu', 'buildFromTemplate'])
    td.replace('../main-process/node_actuator', MockNodeActuator)
    mockHttp = td.replace('http')
    process = td.replace('../main-process/wrappers/process_wrapper')

    const MockBrowserWindow = td.constructor(['on', 'loadURL'])
    mainWindowOnClose = td.matchers.captor()
    td.when(MockBrowserWindow.prototype.on('close', mainWindowOnClose.capture())).thenReturn()

    appOnReady = td.matchers.captor()
    td.when(mockApp.on('ready', appOnReady.capture())).thenReturn(mockApp)

    ipcMainOnIpLookup = td.matchers.captor()
    td.when(mockIpcMain.on('ip-lookup', ipcMainOnIpLookup.capture())).thenReturn(mockIpcMain)

    ipcMainOnChangeNodeState = td.matchers.captor()
    td.when(mockIpcMain.on('change-node-state', ipcMainOnChangeNodeState.capture())).thenReturn(mockIpcMain)

    td.replace('electron', {
      app: mockApp,
      BrowserWindow: MockBrowserWindow,
      dialog: mockDialog,
      ipcMain: mockIpcMain,
      Menu: mockMenu
    })
    require('../main')
  })

  afterEach(() => {
    td.reset()
  })

  describe('ip lookup', () => {
    let event, mockRequest, mockResponse

    beforeEach(() => {
      event = {}
      mockRequest = td.object(['abort', 'on'])
      mockResponse = td.object(['on'])
      td.when(mockHttp.get({ 'host': 'api.ipify.org', 'port': 80, 'path': '/', 'timeout': 1000 }, td.callback(mockResponse)))
        .thenReturn(mockRequest)
    })

    describe('successful', () => {
      beforeEach(() => {
        td.when(mockResponse.on('data')).thenCallback('1.3.2.4')
        td.when(mockResponse.on('end')).thenCallback()
        ipcMainOnIpLookup.value(event)
      })

      it('returns the ip', () => {
        expect(event.returnValue).toBe('1.3.2.4')
      })
    })

    describe('timeout', () => {
      beforeEach(() => {
        td.when(mockRequest.on('timeout')).thenCallback()
        ipcMainOnIpLookup.value(event)
      })

      it('aborts the request', () => {
        td.verify(mockRequest.abort())
      })
    })

    describe('error', () => {
      beforeEach(() => {
        td.when(mockRequest.on('error')).thenCallback("things didn't work out")
        ipcMainOnIpLookup.value(event)
      })

      it('returns empty string', () => {
        expect(event.returnValue).toBe('')
      })
    })
  })

  describe('change-node-state', () => {
    let command
    let event = { returnValue: null }
    let arg = ['inconsequential']

    beforeEach(() => {
      appOnReady.value()
    })

    describe('when command is turn-off', () => {
      beforeEach(() => {
        command = 'turn-off'
      })

      describe('and off succeeds', () => {
        beforeEach(async () => {
          td.when(MockNodeActuator.prototype.off()).thenResolve('Off')
          await ipcMainOnChangeNodeState.value(event, command, arg)
        })

        it("returns 'Off'", () => {
          expect(event.returnValue).toBe('Off')
        })
      })

      describe('and off fails', () => {
        beforeEach(async () => {
          td.when(MockNodeActuator.prototype.off()).thenReject(Error('off failed'))
          await ipcMainOnChangeNodeState.value(event, command, arg)
        })

        it("returns 'Invalid'", () => {
          expect(event.returnValue).toBe('Invalid')
        })
      })
    })

    describe('when command is serve', () => {
      beforeEach(() => {
        command = 'serve'
      })

      describe('and serving succeeds', () => {
        beforeEach(async () => {
          td.when(MockNodeActuator.prototype.serving(arg)).thenResolve('Serving')
          await ipcMainOnChangeNodeState.value(event, command, arg)
        })

        it("returns 'Serving'", () => {
          expect(event.returnValue).toBe('Serving')
        })
      })

      describe('and serving fails', () => {
        beforeEach(async () => {
          td.when(MockNodeActuator.prototype.serving(arg)).thenReject(Error('serving failed'))
          await ipcMainOnChangeNodeState.value(event, command, arg)
        })

        it("returns 'Invalid'", () => {
          expect(event.returnValue).toBe('Invalid')
        })
      })
    })

    describe('when command is consume', () => {
      beforeEach(() => {
        command = 'consume'
      })

      describe('and consuming succeeds', () => {
        beforeEach(async () => {
          td.when(MockNodeActuator.prototype.consuming(arg)).thenResolve('Consuming')
          await ipcMainOnChangeNodeState.value(event, command, arg)
        })

        it("returns 'Consuming'", () => {
          expect(event.returnValue).toBe('Consuming')
        })
      })

      describe('and consuming fails', () => {
        beforeEach(async () => {
          td.when(MockNodeActuator.prototype.consuming(arg)).thenReject(Error('consuming failed'))
          await ipcMainOnChangeNodeState.value(event, command, arg)
        })

        it("returns 'Invalid'", () => {
          expect(event.returnValue).toBe('Invalid')
        })
      })
    })
  })

  describe('shutting down', () => {
    describe('on mac', () => {
      beforeEach(() => {
        process.platform = 'darwin'
      })
      it('Menu is set up', () => {
        appOnReady.value()

        td.verify(mockMenu.buildFromTemplate([
          {
            label: undefined,
            submenu: [
              { role: 'quit' }
            ]
          },
          {
            label: 'Edit',
            submenu: [
              { role: 'undo' },
              { role: 'redo' },
              { type: 'separator' },
              { role: 'cut' },
              { role: 'copy' },
              { role: 'paste' },
              { role: 'pasteandmatchstyle' },
              { role: 'delete' },
              { role: 'selectall' }
            ]
          }
        ]))
        td.verify(mockMenu.setApplicationMenu(td.matchers.anything()))
      })
    })

    describe('on other platforms', () => {
      beforeEach(() => {
        process.platform = 'anything other than elephants is irelephant'
      })

      it('Menu is not set up', () => {
        appOnReady.value()

        td.verify(mockMenu.buildFromTemplate(td.matchers.anything()), { times: 0 })
        td.verify(mockMenu.setApplicationMenu(td.matchers.anything()), { times: 0 })
      })
    })

    describe('successfully', () => {
      beforeEach(async () => {
        td.when(MockNodeActuator.prototype.shutdown()).thenResolve()
        appOnReady.value()

        await mainWindowOnClose.value(mockEvent)
      })

      it('prevents the default close event behavior', async () => {
        td.verify(mockEvent.preventDefault(), { times: 1 })
      })

      it('quits the app', async () => {
        td.verify(mockApp.quit(), { times: 1 })
      })

      describe('when the app is already quitting', () => {
        beforeEach(async () => {
          await mainWindowOnClose.value(mockEvent)
        })

        it('does not prevent the default close event behavior', () => {
          td.verify(mockEvent.preventDefault(), { times: 1 })
        })

        it('does not call quit again', () => {
          td.verify(mockApp.quit(), { times: 1 })
        })
      })
    })

    describe('unsuccessfully', () => {
      beforeEach(async () => {
        td.when(MockNodeActuator.prototype.shutdown()).thenReject('beggin for help')
        appOnReady.value()

        await mainWindowOnClose.value(mockEvent)
      })

      it('shows an error dialog', () => {
        td.verify(mockDialog.showErrorBox(
          'Error shutting down Substratum Node.',
          'Could not shut down Substratum Node.  You may need to kill it manually.\n\nReason: "beggin for help"'))
      })

      it('quits the app', () => {
        td.verify(mockApp.quit())
      })

      describe('when the app is already quitting', () => {
        beforeEach(async () => {
          await mainWindowOnClose.value(mockEvent)
        })

        it('does not prevent the default close event behavior', () => {
          td.verify(mockEvent.preventDefault(), { times: 1 })
        })

        it('does not call quit again', () => {
          td.verify(mockApp.quit(), { times: 1 })
        })
      })
    })
  })
})
