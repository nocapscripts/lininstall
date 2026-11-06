/* LinInstall frontend. All system logic lives in Rust (src-tauri/src); this file only renders. */
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const api = {
  getData: () => invoke("get_data"),
  setPm: (choice) => invoke("set_pm", { choice }),
  runTask: (id) => invoke("run_task", { id }),
  listPackages: () => invoke("list_packages"),
  removePackages: (sel, dry) => invoke("remove_packages", { sel, dry }),
  onLog: (cb) => listen("log", (e) => cb(e.payload)),
  version: () => window.__TAURI__.app.getVersion(),
  async copy(text) {
    try { await navigator.clipboard.writeText(text); }
    catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.append(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
  },
};

let data = null;
let current = "home";
let busy = false;
let pending = null;

const $ = (id) => document.getElementById(id);
const h = (tag, props = {}, ...kids) => {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else el[k] = v;
  }
  kids.flat().forEach((c) => c != null && el.append(c.nodeType ? c : document.createTextNode(c)));
  return el;
};

/* ---------- Tailwind class sets (full class names so the scanner finds them) ---------- */
const CLS = {
  navBase: "flex items-center gap-2.5 text-left px-3 py-2.5 rounded-xl whitespace-nowrap shrink-0",
  navOn: "bg-gradient-to-br from-accent to-accent2 text-white",
  navOff: "text-muted hover:bg-panel2 hover:text-ink",
  group: "mt-[18px] mb-2 text-[11px] font-bold tracking-[.08em] uppercase text-muted",
  grid: "grid grid-cols-[repeat(auto-fill,minmax(min(100%,280px),1fr))] gap-2.5",
  card: "bg-panel border border-line rounded-2xl p-3.5 flex flex-col gap-2 hover:border-[#3a4155]",
  cardOff: "bg-panel border border-line rounded-2xl p-3.5 flex flex-col gap-2 opacity-40",
  hero: "bg-gradient-to-br from-[#1b2040] to-[#2a1b45] border border-[#3a3f78] rounded-2xl p-3.5 flex flex-col gap-2",
  desc: "m-0 flex-1 text-xs leading-snug text-muted",
  tag: "text-[10px] px-[7px] py-0.5 rounded-full bg-panel2",
  primary: "bg-gradient-to-br from-accent to-accent2 text-white rounded-[9px] px-4 py-1.5 font-semibold disabled:opacity-40 disabled:cursor-not-allowed",
  primaryDanger: "bg-gradient-to-br from-red-500 to-orange-500 text-white rounded-[9px] px-4 py-1.5 font-semibold",
  ghost: "border border-line text-muted rounded-lg px-3 py-1.5 hover:text-ink hover:border-muted",
  kv: "grid grid-cols-[110px_1fr] gap-x-2.5 gap-y-1.5 text-[13px]",
  pkgbar: "flex flex-wrap gap-2 items-center mb-3 sticky top-0 bg-bg py-1.5 z-[1]",
  pkgfilter: "flex-1 min-w-[140px] px-3 py-2 rounded-xl border border-line bg-panel2 text-ink outline-none focus:border-accent",
  pkglist: "grid grid-cols-[repeat(auto-fill,minmax(min(100%,260px),1fr))] gap-1.5",
  pkg: "flex items-center gap-2.5 px-3 py-2 bg-panel border border-line rounded-xl cursor-pointer hover:border-[#3a4155]",
};
const split = (s) => s.split(" ");

/* ---------- log (batched per frame so heavy output never blocks the UI) ---------- */
const logEl = $("log");
const logNode = document.createTextNode("Output will appear here…");
logEl.append(logNode);
const MAX_LOG = 400000;
let logStarted = false, logBuf = "", logRaf = 0;

function flushLog() {
  logRaf = 0;
  if (!logStarted) { logNode.data = ""; logStarted = true; }
  logNode.appendData(logBuf);
  logBuf = "";
  if (logNode.length > MAX_LOG) logNode.deleteData(0, logNode.length - MAX_LOG / 2);
  logEl.scrollTop = logEl.scrollHeight;
}
const appendLog = (t) => {
  logBuf += t;
  if (!logRaf) logRaf = requestAnimationFrame(flushLog);
};
api.onLog(appendLog);
$("clearlog").onclick = () => { logBuf = ""; logNode.data = ""; logStarted = true; };
$("copylog").onclick = () => api.copy(logNode.data);

/* ---------- modal ---------- */
function showModal(on) {
  $("modal").classList.toggle("hidden", !on);
  $("modal").classList.toggle("flex", on);
}
function askRun(task) {
  pending = task;
  $("m-title").textContent = task.name;
  $("m-desc").textContent = task.desc;
  $("m-cmd").textContent = (task.root ? "# runs as root (password prompt)\n" : "# runs as your user\n") + task.cmd;
  $("m-tags").replaceChildren(...tags(task));
  $("m-ok").className = task.danger ? CLS.primaryDanger : CLS.primary;
  showModal(true);
}
const closeModal = () => { showModal(false); pending = null; };
$("m-cancel").onclick = closeModal;
document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeModal(); });
$("m-ok").onclick = async () => {
  const task = pending;
  showModal(false);
  if (!task || busy) return;
  busy = true;
  $("status").textContent = `Running: ${task.name}…`;
  document.querySelectorAll("button.run").forEach((b) => (b.disabled = true));
  try {
    await (task.run ? task.run() : api.runTask(task.id));
  } catch (e) {
    appendLog(`\nError: ${e}\n`);
  }
  busy = false;
  $("status").textContent = "";
  document.querySelectorAll("button.run").forEach((b) => (b.disabled = false));
};

/* ---------- rendering ---------- */
const tags = (t) => [
  t.root ? h("span", { class: `${CLS.tag} text-warn` }, "root") : null,
  t.reboot ? h("span", { class: `${CLS.tag} text-accent` }, "reboot") : null,
  t.danger ? h("span", { class: `${CLS.tag} text-bad` }, "careful") : null,
].filter(Boolean);

const kv = (rows) => h("div", { class: CLS.kv }, rows.flatMap(([k, v]) => [h("span", { class: "text-muted" }, k), h("span", { class: "min-w-0 break-words" }, v)]));

const taskCard = (t) =>
  h("div", { class: t.cmd ? CLS.card : CLS.cardOff },
    h("div", { class: "flex justify-between items-start gap-2.5" },
      h("b", { class: "text-sm" }, t.name),
      h("button", { class: `run shrink-0 ${CLS.primary}`, disabled: !t.cmd || busy, onclick: () => askRun(t) }, t.cmd ? "Run" : "N/A")),
    h("p", { class: CLS.desc }, t.desc),
    h("div", { class: "flex gap-1.5 flex-wrap" }, tags(t), t.cmd ? null : h("span", { class: `${CLS.tag} text-muted` }, "not supported on this distro")));

function renderTasks(tasks, el) {
  const groups = [...new Set(tasks.map((t) => t.group))];
  groups.forEach((g) => {
    if (g) el.append(h("div", { class: CLS.group }, g));
    el.append(h("div", { class: CLS.grid }, tasks.filter((t) => t.group === g).map(taskCard)));
  });
}

function gpuCard(g, info) {
  const rec = g.vendor === "NVIDIA"
    ? g.branch ? (g.branch === "current" ? "Current NVIDIA driver" : `Legacy ${g.branch} driver`) : "Unknown generation"
    : g.vendor === "AMD" || g.vendor === "Intel" ? "Open-source Mesa drivers (already in the kernel)" : "—";
  const rows = [["Vendor", g.vendor]];
  if (g.generation) rows.push(["Generation", g.generation]);
  rows.push(["Recommended", rec]);
  if (g.vendor === "NVIDIA") rows.push(["Loaded driver", info.nvidiaDriver ? info.nvidiaDriver + " (nvidia-smi OK)" : "none / nouveau"]);
  return h("div", { class: CLS.card }, h("b", { class: "text-sm" }, g.name), kv(rows));
}

const findTask = (id) => data.categories.flatMap((c) => c.tasks).find((t) => t.id === id);

function renderHome(el) {
  const i = data.info;
  el.append(h("div", { class: CLS.grid },
    h("div", { class: CLS.card }, h("b", { class: "text-sm" }, "System"),
      kv([["Distro", i.distro], ["Kernel", i.kernel], ["CPU", i.cpu], ["RAM", i.ramGB + " GB"],
        ["Session", `${i.session} / ${i.desktop}`], ["Secure Boot", i.secureBoot], ["Pkg manager", i.pm || "not found"]])),
    h("div", { class: CLS.hero }, h("h3", { class: "m-0 text-base font-semibold" }, "GPU drivers"),
      h("p", { class: CLS.desc }, i.nvidia
        ? `Detected ${i.nvidia.name}${i.nvidia.generation ? " (" + i.nvidia.generation + ")" : ""}. ` +
          (i.nvidia.branch === "580xx" ? "This generation needs the legacy 580xx branch — newer drivers dropped support for it." : "The recommended driver can be installed in one click.")
        : "No NVIDIA GPU detected."),
      i.nvidia ? h("button", { class: `run self-start ${CLS.primary}`, disabled: busy, onclick: () => askRun(findTask("nv-auto")) }, "Install recommended driver") : null)));
  el.append(h("div", { class: CLS.group }, "Graphics devices"));
  el.append(h("div", { class: CLS.grid }, i.gpus.length ? i.gpus.map((g) => gpuCard(g, i)) : h("div", { class: CLS.card }, i.hasLspci ? "No GPU found." : "lspci not found — install pciutils to detect your GPU.")));
  el.append(h("div", { class: CLS.group }, "Quick actions"));
  el.append(h("div", { class: CLS.grid }, ["update", "gpu-status", "firewall", "flathub"].map((id) => taskCard(findTask(id)))));
}

async function renderRemove(el) {
  $("title").textContent = "Remove installed apps";
  const box = h("p", { class: "text-muted" }, "Loading installed packages…");
  el.append(box);
  let packages;
  try {
    packages = await api.listPackages();
  } catch (e) {
    box.textContent = `Could not list packages: ${e}`;
    return;
  }
  if (current !== "remove") return; // user navigated away while loading
  const { native, flatpak } = packages;
  const selected = new Set();
  const count = h("span", { class: `${CLS.tag} text-muted` }, "0 selected");
  const filter = h("input", { type: "search", placeholder: "Filter packages…", class: CLS.pkgfilter });
  const list = h("div", { class: CLS.pkglist });

  const selection = () => ({
    native: [...selected].filter((k) => k.startsWith("n:")).map((k) => k.slice(2)),
    flatpak: [...selected].filter((k) => k.startsWith("f:")).map((k) => k.slice(2)),
  });

  const draw = () => {
    list.replaceChildren();
    const q = filter.value.toLowerCase();
    const rows = [
      ...flatpak.map((f) => ({ key: "f:" + f.id, label: f.name, sub: `${f.id} · Flatpak` })),
      ...native.map((n) => ({ key: "n:" + n, label: n, sub: "package" })),
    ].filter((r) => (r.label + " " + r.sub).toLowerCase().includes(q));
    rows.slice(0, 400).forEach((r) =>
      list.append(h("label", { class: CLS.pkg },
        h("input", { type: "checkbox", class: "accent-accent", checked: selected.has(r.key), onchange: (e) => {
          e.target.checked ? selected.add(r.key) : selected.delete(r.key);
          count.textContent = `${selected.size} selected`;
        } }),
        h("span", { class: "min-w-0 break-all" }, r.label), h("small", { class: "ml-auto text-[11px] text-muted" }, r.sub))));
    if (rows.length > 400) list.append(h("p", { class: "text-muted" }, "Showing the first 400. Use the filter to narrow down."));
    if (!rows.length) list.append(h("p", { class: "text-muted" }, "No packages found."));
  };
  filter.addEventListener("input", draw);

  const preview = h("button", { class: CLS.ghost, onclick: async () => {
    const s = selection();
    if (!s.native.length && !s.flatpak.length) return;
    try { await api.removePackages(s, true); } catch (e) { appendLog(`\nError: ${e}\n`); }
  } }, "Preview");
  const remove = h("button", { class: CLS.primaryDanger, onclick: () => {
    const s = selection();
    const all = [...s.native, ...s.flatpak];
    if (!all.length) return;
    askRun({
      name: `Remove ${all.length} package(s)`,
      desc: "Dependencies that are no longer needed may be removed too. Use Preview first to see exactly what goes.",
      root: s.native.length > 0 && !["brew", "nix", "yay", "paru"].includes(data.info.pm), reboot: false, danger: true,
      cmd: all.join("\n"),
      run: () => api.removePackages(s, false),
    });
  } }, "Remove selected");

  box.replaceWith(h("div", {}, h("div", { class: CLS.pkgbar }, filter, count, preview, remove), list));
  draw();
}

function styleNav() {
  document.querySelectorAll("#nav button").forEach((b) => {
    const on = b.dataset.id === current;
    b.classList.remove(...split(on ? CLS.navOff : CLS.navOn));
    b.classList.add(...split(on ? CLS.navOn : CLS.navOff));
    if (on) b.scrollIntoView({ block: "nearest", inline: "nearest" });
  });
}

function render() {
  const el = $("content");
  el.replaceChildren();
  styleNav();
  const q = $("search").value.trim().toLowerCase();
  if (q) {
    $("title").textContent = `Search: ${q}`;
    const all = data.categories.flatMap((c) => c.tasks.map((t) => ({ ...t, group: c.name })));
    const hits = all.filter((t) => (t.name + " " + t.desc + " " + t.group).toLowerCase().includes(q));
    hits.length ? renderTasks(hits, el) : el.append(h("p", { class: "text-muted" }, "Nothing found."));
    return;
  }
  if (current === "home") { $("title").textContent = "Overview"; renderHome(el); return; }
  if (current === "remove") { renderRemove(el); return; }
  const cat = data.categories.find((c) => c.id === current);
  $("title").textContent = cat.name;
  renderTasks(cat.tasks, el);
}

function buildNav() {
  const nav = $("nav");
  nav.replaceChildren();
  const add = (id, label) => {
    const b = h("button", { class: CLS.navBase, onclick: () => go(id) }, label);
    b.dataset.id = id;
    nav.append(b);
  };
  add("home", "🏠 Overview");
  data.categories.forEach((c) => add(c.id, `${c.icon} ${c.name}`));
  add("remove", "🗑️ Remove apps");
}
function go(id) { current = id; $("search").value = ""; render(); }

// Re-render at most once per frame while typing in the search box
let searchRaf = 0;
$("search").addEventListener("input", () => {
  if (!data || searchRaf) return;
  searchRaf = requestAnimationFrame(() => { searchRaf = 0; render(); });
});

function buildPmSelect() {
  const sel = $("pm"), i = data.info;
  sel.replaceChildren(
    h("option", { value: "auto" }, `Auto (${i.defaultPm || "none found"})`),
    ...i.allPms.map((p) => h("option", { value: p }, i.availablePms.includes(p) ? p : `${p} (not installed)`)));
  sel.value = i.pmChoice;
  $("pm-hint").textContent = i.pm && !i.availablePms.includes(i.pm)
    ? `${i.pm} is not installed - tasks will fail until you install it.` : "";
}
$("pm").addEventListener("change", async (e) => {
  if (busy) { e.target.value = data.info.pmChoice; return; }
  try {
    data = await api.setPm(e.target.value);
    buildPmSelect();
    render();
  } catch (err) {
    appendLog(`\nError: ${err}\n`);
  }
});

(async () => {
  $("title").textContent = "Overview";
  $("content").append(h("p", { class: "text-muted" }, "Detecting your system…"));
  api.version().then((v) => ($("version").textContent = `v${v}`)).catch(() => {});
  try {
    data = await api.getData();
  } catch (e) {
    $("content").replaceChildren(h("p", { class: "text-bad" }, `Failed to load system info: ${e}`));
    return;
  }
  buildPmSelect();
  $("distro").textContent = `${data.info.distro}\nKernel ${data.info.kernel}`;
  buildNav();
  render();
})();
