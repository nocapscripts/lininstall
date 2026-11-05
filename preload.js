const { contextBridge, ipcRenderer } = require("electron");
contextBridge.exposeInMainWorld("api", {
  getData: () => ipcRenderer.invoke("get-data"),
  getVersion: () => ipcRenderer.invoke("get-version"),
  setPm: (c) => ipcRenderer.invoke("set-pm", c),
  refresh: () => ipcRenderer.invoke("refresh"),
  runTask: (id) => ipcRenderer.invoke("run-task", id),
  copy: (t) => ipcRenderer.invoke("copy", t),
  listPackages: () => ipcRenderer.invoke("list-packages"),
  removePackages: (sel, dry) => ipcRenderer.invoke("remove-packages", sel, dry),
  onLog: (cb) => ipcRenderer.on("log", (_e, text) => cb(text)),
});
