const { contextBridge, ipcRenderer } = require("electron");
contextBridge.exposeInMainWorld("api", {
  getData: () => ipcRenderer.invoke("get-data"),
  refresh: () => ipcRenderer.invoke("refresh"),
  runTask: (id) => ipcRenderer.invoke("run-task", id),
  copy: (t) => ipcRenderer.invoke("copy", t),
  listPackages: () => ipcRenderer.invoke("list-packages"),
  removePackages: (sel, dry) => ipcRenderer.invoke("remove-packages", sel, dry),
  onLog: (cb) => ipcRenderer.on("log", (_e, text) => cb(text)),
});
