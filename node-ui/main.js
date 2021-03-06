const { app, dialog, BrowserWindow, ipcMain, Menu } = require('electron')
const path = require('path')
const url = require('url')
const process = require('./main-process/wrappers/process_wrapper')
const http = require('http')

const NodeActuator = require('./main-process/node_actuator')

const Invalid = 'Invalid'

let mainWindow
let nodeActuator

function createWindow () {
  // Mac needs special menu entries for clipboard functionality
  if (process.platform === 'darwin') {
    Menu.setApplicationMenu(Menu.buildFromTemplate([
      {
        label: app.getName(),
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
  }

  mainWindow = new BrowserWindow({
    width: 620,
    height: 560,
    show: true,
    frame: true,
    backgroundColor: '#383839',
    fullscreenable: false,
    resizable: false,
    transparent: false,
    webPreferences: {
      backgroundThrottling: false
    }
  })

  // load the dist folder from Angular
  mainWindow.loadURL(
    url.format({
      pathname: path.join(__dirname, `/dist/index.html`),
      protocol: 'file:',
      slashes: true
    })
  )

  nodeActuator = new NodeActuator(mainWindow.webContents)

  // The following is optional and will open the DevTools:
  // mainWindow.webContents.openDevTools({ mode: 'detach' })

  let quitting = false
  mainWindow.on('close', event => {
    if (!quitting) {
      quitting = true

      event.preventDefault()
      nodeActuator.shutdown()
        .then(() => app.quit())
        .catch((reason) => {
          dialog.showErrorBox(
            'Error shutting down Substratum Node.',
            `Could not shut down Substratum Node.  You may need to kill it manually.\n\nReason: "${reason}"`
          )
          app.quit()
        })
    }
  })

  mainWindow.on('closed', () => {
    mainWindow = null
  })
}

app.on('ready', createWindow)

app.on('window-all-closed', app.quit)

app.on('activate', () => {
  if (mainWindow === null) {
    createWindow()
  }
})

ipcMain.on('ip-lookup', async (event, command, args) => {
  let req = http.get(
    { 'host': 'api.ipify.org', 'port': 80, 'path': '/', 'timeout': 1000 },
    resp => {
      let rawData = ''
      resp.on('data', chunk => { rawData += chunk })
      resp.on('end', () => { event.returnValue = rawData })
    })

  req.on('timeout', () => { req.abort() })
  req.on('error', () => { event.returnValue = '' })
})

ipcMain.on('change-node-state', (event, command, args) => {
  if (command === 'turn-off') {
    assignStatus(event, nodeActuator.off())
  } else if (command === 'serve') {
    assignStatus(event, nodeActuator.serving(args))
  } else if (command === 'consume') {
    assignStatus(event, nodeActuator.consuming(args))
  }
})

let assignStatus = (event, promise) => {
  promise.then(val => {
    event.returnValue = val
  }).catch(() => {
    event.returnValue = Invalid
  })
}
