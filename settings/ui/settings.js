// Glide for Windows 설정 화면.
// 크롬 확장 options.js(커밋 770047b)를 바탕으로 다음을 바꿨다.
// - 저장소: chrome.storage 대신 Tauri 명령(invoke) → 설정 창(Rust) → 파이프 → 코어가 검증·저장
// - 동작 목록: 코어의 동작 표(Windows 단축키 안내, Windows 전용 "창과 미디어" 묶음)로 바꿈
// - 상태 칸(코어 실행·관리자 창 지원·훅 처리 시간), Windows 동작 칸(길게 누르기·전체 화면·예외 앱)
// - 백업: 파일 선택·저장은 Rust 쪽 대화상자가 하고, 화면은 결과만 받는다
"use strict";

const { invoke } = window.__TAURI__.core;
const { actions, arrows, Gesture, Trail, actionLabel } = GestureKit;
const $ = selector => document.querySelector(selector);
let settings;
let catalog = [];
let coreRunning = false;
let editingIndex;
let recorded = "";
let recording;
let recordingPointer;
let draftRule;
let bindingCount = 0;
const recordTrail = new Trail($("#record-canvas"));

// 코어의 동작 표를 확장 코드(GestureKit)가 쓰는 동작 목록에 반영한다.
// 확장과 같은 ID는 라벨을 유지하고 단축키 안내만 Windows 기준으로 바꾸며, Windows 전용 동작은 새 묶음에 넣는다.
function applyCatalog(entries) {
  catalog = entries;
  if (!GestureKit.actionGroups.some(group => group.id === "window")) {
    GestureKit.actionGroups.push({ id: "window", label: "창과 미디어 (Windows)" });
  }
  for (const entry of entries) {
    const group = actions[entry.id]?.group ?? "window";
    actions[entry.id] = { label: entry.label, shortcut: entry.hint, group, supported: entry.supported };
  }
}

// 직접 입력한 단축키가 기본 동작과 같은 키 조합이면 그 동작 이름을 알려 준다(코어 HUD와 같은 규칙).
function actionForKeys(text) {
  return catalog.find(entry => entry.keys === text);
}

function status(text, error = false) {
  $("#status").textContent = text;
  $("#status").dataset.error = error;
}

function backupStatus(text, error = false) {
  const node = $("#backup-status");
  node.textContent = text;
  node.dataset.error = error;
  node.hidden = !text;
}

function clearBackupStatus() {
  backupStatus("");
}

// 파일 이름에 쓰는 현지 날짜(YYYY-MM-DD).
function dateStamp() {
  const date = new Date();
  const pad = value => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

// 저장할 수 있는 상태(코어 실행 중)에서만 저장 버튼을 켠다.
function dirty() {
  $("#save").dataset.dirty = "true";
  $("#save").disabled = !coreRunning;
  status(coreRunning ? "변경사항을 저장해 주세요." : "Glide가 실행 중이 아니라 저장할 수 없습니다. 위에서 Glide를 시작해 주세요.", !coreRunning);
  clearBackupStatus();
}

function isDirty() {
  return $("#save").dataset.dirty === "true";
}

function bindingControls(rule, onChange, name) {
  const container = document.createElement("div");
  container.className = "binding";
  const select = document.createElement("select");
  select.setAttribute("aria-label", `${name} 실행 방식`);
  // 새 제스처는 먼저 동작을 고르도록 비워 둔다.
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = "동작을 선택하세요";
  placeholder.disabled = true;
  select.append(placeholder);
  for (const group of GestureKit.actionGroups) {
    const optgroup = document.createElement("optgroup");
    optgroup.label = group.label;
    for (const [id, action] of Object.entries(actions)) {
      if (action.group !== group.id) continue;
      const option = document.createElement("option");
      option.value = id;
      option.textContent = actionLabel(id);
      optgroup.append(option);
    }
    select.append(optgroup);
  }
  const customGroup = document.createElement("optgroup");
  customGroup.label = "직접 지정";
  const custom = document.createElement("option");
  custom.value = "keyboard";
  custom.textContent = "직접 단축키 입력";
  customGroup.append(custom);
  select.append(customGroup);
  select.value = rule.action;
  const controls = document.createElement("div");
  controls.className = "shortcut-controls";
  const input = document.createElement("input");
  input.type = "text";
  input.maxLength = 80;
  input.placeholder = "예: Ctrl+T, Alt+Left, F";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.value = rule.shortcut || "";
  input.setAttribute("aria-label", `${name} 단축키`);
  const capture = document.createElement("button");
  capture.type = "button";
  capture.textContent = "키로 입력";
  capture.setAttribute("aria-label", `${name} 키로 입력`);
  capture.setAttribute("aria-pressed", "false");
  const hint = document.createElement("p");
  hint.className = "binding-hint";
  hint.setAttribute("role", "status");
  hint.id = `binding-hint-${++bindingCount}`;
  select.setAttribute("aria-describedby", hint.id);
  let listening = false;

  function preview() {
    const keyboard = rule.action === "keyboard";
    controls.hidden = !keyboard;
    hint.hidden = false;
    if (!rule.action) {
      hint.textContent = "실행할 동작을 선택하거나 단축키를 직접 입력하세요.";
      hint.dataset.error = "false";
      return;
    }
    if (!keyboard) {
      // 공통 단축키가 없어 Windows에서 실행되지 않는 동작(탭 복제·음소거)은 고른 순간 알려 준다.
      if (actions[rule.action]?.supported === false) {
        hint.textContent = "이 동작은 Windows에 공통 단축키가 없어 실행되지 않습니다. 브라우저 확장에서만 동작합니다.";
        hint.dataset.error = "true";
      } else {
        hint.hidden = true;
      }
      return;
    }
    if (!input.value.trim()) {
      hint.textContent = "단축키를 입력하거나 ‘키로 입력’을 누르세요.";
      hint.dataset.error = "false";
      return;
    }
    try {
      const shortcut = GestureKit.parseShortcut(rule.shortcut);
      const same = actionForKeys(shortcut.text);
      hint.textContent = same ? `${shortcut.text} → ${same.label}과 같은 키 입력` :
        `${shortcut.text} → 제스처를 시작한 창에 이 키를 보냅니다.`;
      hint.dataset.error = "false";
    } catch (error) {
      hint.textContent = error.message;
      hint.dataset.error = "true";
    }
  }
  function stopCapture() {
    listening = false;
    capture.textContent = "키로 입력";
    capture.setAttribute("aria-pressed", "false");
    preview();
  }
  select.addEventListener("change", () => {
    rule.action = select.value;
    if (rule.action === "keyboard") rule.shortcut = input.value;
    else delete rule.shortcut;
    stopCapture();
    onChange();
    if (rule.action === "keyboard") input.focus();
  });
  input.addEventListener("input", () => { rule.shortcut = input.value; preview(); onChange(); });
  input.addEventListener("blur", () => {
    try { input.value = rule.shortcut = GestureKit.parseShortcut(input.value).text; } catch {}
    preview();
  });
  capture.addEventListener("click", () => {
    if (listening) { stopCapture(); return; }
    listening = true;
    capture.textContent = "키를 누르세요…";
    capture.setAttribute("aria-pressed", "true");
    hint.textContent = "키 조합을 누르세요. Esc는 취소. Windows가 먼저 가로채는 조합(Win 키 등)은 입력란에 Cmd+D처럼 적어 주세요.";
    hint.dataset.error = "false";
  });
  capture.addEventListener("keydown", event => {
    if (!listening) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape" && !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey) { stopCapture(); return; }
    try {
      const shortcut = GestureKit.shortcutFromEvent(event);
      if (!shortcut) return;
      input.value = rule.shortcut = shortcut;
      stopCapture();
      onChange();
    } catch (error) { hint.textContent = error.message; hint.dataset.error = "true"; }
  });
  capture.addEventListener("blur", stopCapture);
  controls.append(input, capture);
  container.append(select, controls, hint);
  preview();
  return container;
}

// 제스처 방향을 작은 그림으로 그린 SVG. 장식용이므로 낭독기에서는 숨긴다.
const svgNS = "http://www.w3.org/2000/svg";
function glyphElement(sequence, size = 40) {
  const { path, head, start } = GestureKit.gestureGlyph(sequence, size);
  const svg = document.createElementNS(svgNS, "svg");
  svg.setAttribute("viewBox", `0 0 ${size} ${size}`);
  svg.setAttribute("fill", "none");
  svg.setAttribute("aria-hidden", "true");
  for (const d of [path, head]) {
    const line = document.createElementNS(svgNS, "path");
    line.setAttribute("d", d);
    line.setAttribute("stroke", "currentColor");
    line.setAttribute("stroke-width", String(size * 0.065));
    line.setAttribute("stroke-linecap", "round");
    line.setAttribute("stroke-linejoin", "round");
    svg.append(line);
  }
  const dot = document.createElementNS(svgNS, "circle");
  dot.setAttribute("cx", start[0]);
  dot.setAttribute("cy", start[1]);
  dot.setAttribute("r", String(size * 0.065));
  dot.setAttribute("fill", "currentColor");
  svg.append(dot);
  return svg;
}

// 감도 미리보기: 점선 원의 반지름을 실제 인식 거리(px)와 같게 맞춘다.
function renderThreshold() {
  $("#distance").value = `${settings.threshold} px`;
  $("#sens-ring").style.setProperty("--r", `${settings.threshold}px`);
}

function renderHold() {
  $("#hold-value").value = `${(settings.windows.holdToPassMs / 1000).toFixed(2)}초`;
}

function renderExcluded() {
  const list = $("#excluded");
  list.replaceChildren();
  settings.windows.excludedProcesses.forEach((name, index) => {
    const item = document.createElement("li");
    item.append(name);
    const remove = document.createElement("button");
    remove.type = "button";
    remove.setAttribute("aria-label", `${name} 예외 해제`);
    remove.innerHTML = '<svg width="14" height="14" viewBox="0 0 18 18" fill="none" aria-hidden="true"><path d="m4.5 4.5 9 9m0-9-9 9" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>';
    remove.addEventListener("click", () => {
      settings.windows.excludedProcesses.splice(index, 1);
      renderExcluded();
      dirty();
      $("#exclude-input").focus();
    });
    item.append(remove);
    list.append(item);
  });
}

function render() {
  $("#enabled").checked = settings.enabled;
  $("#threshold").value = settings.threshold;
  renderThreshold();
  $("#hold").value = settings.windows.holdToPassMs;
  renderHold();
  $("#auto-pause").checked = settings.windows.autoPauseFullscreen;
  renderExcluded();
  $("#rule-count").textContent = settings.rules.length ? `${settings.rules.length}개` : "";
  $("#empty").hidden = settings.rules.length !== 0;
  $("#add").disabled = settings.rules.length >= 64;
  $("#rules").replaceChildren();
  settings.rules.forEach((rule, index) => {
    const row = document.createElement("tr");
    const gestureCell = row.insertCell();
    const button = document.createElement("button");
    button.type = "button";
    button.className = "gesture-button";
    const glyph = document.createElement("span");
    glyph.className = "glyph";
    glyph.append(glyphElement(rule.gesture));
    const label = document.createElement("span");
    label.className = "gesture-arrows";
    label.textContent = arrows(rule.gesture);
    button.append(glyph, label);
    button.setAttribute("aria-label", `${arrows(rule.gesture)} 제스처 다시 그리기`);
    button.addEventListener("click", () => openRecorder(index));
    gestureCell.append(button);
    row.insertCell().append(bindingControls(rule, dirty, arrows(rule.gesture)));
    const remove = document.createElement("button");
    remove.className = "icon-button";
    remove.type = "button";
    remove.innerHTML = '<svg width="18" height="18" viewBox="0 0 18 18" fill="none" aria-hidden="true"><path d="M3.5 5h11M7.5 2.8h3M5 5l.7 9.2a1.5 1.5 0 0 0 1.5 1.3h3.6a1.5 1.5 0 0 0 1.5-1.3L13 5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';
    remove.setAttribute("aria-label", `${arrows(rule.gesture)} 제스처 삭제`);
    remove.addEventListener("click", () => {
      settings.rules.splice(index, 1);
      render();
      dirty();
      const next = $("#rules").rows[Math.min(index, settings.rules.length - 1)];
      (next?.querySelector(".icon-button") || $("#add")).focus();
    });
    row.insertCell().append(remove);
    $("#rules").append(row);
  });
}

// ── 상태 칸 ──
function renderCore(core) {
  const wasRunning = coreRunning;
  coreRunning = !!core.running;
  const card = $("#core");
  card.dataset.state = coreRunning ? "running" : "stopped";
  $("#start-core").hidden = coreRunning;
  const facts = [];
  if (coreRunning) {
    const s = core.status;
    $("#core-title").textContent = s.active ? "제스처 사용 중" :
      !s.runtimeEnabled ? "트레이에서 잠시 꺼 둠" : s.fullscreenPaused ? "전체 화면이라 일시 중지" : "설정에서 꺼 둠";
    $("#core-detail").textContent = `Glide ${s.version} · 알림 영역(트레이)에서 실행 중`;
    facts.push(s.uiAccess ? ["관리자 권한 창 지원: 켜짐", "ok"] : ["관리자 권한 창 지원: 꺼짐", "warn"]);
    const p99 = s.hookCalls === 0 ? "기록 없음" : s.hookOver10ms ? "10ms 이상" : `${s.hookP99Micros}µs 미만`;
    facts.push([`훅 처리 시간 p99: ${p99}`, s.hookOver10ms ? "warn" : ""]);
    if (s.reinstalls) facts.push([`훅 재설치 ${s.reinstalls}회`, ""]);
  } else {
    $("#core-title").textContent = "Glide가 실행 중이 아닙니다";
    $("#core-detail").textContent = core.error || "제스처를 쓰려면 Glide를 시작하세요. 설정은 Glide가 실행 중일 때 저장할 수 있습니다.";
  }
  $("#core-facts").replaceChildren(...facts.map(([text, tone]) => {
    const item = document.createElement("li");
    item.textContent = text;
    if (tone) item.dataset.tone = tone;
    return item;
  }));
  if (wasRunning !== coreRunning && isDirty()) dirty();
  return !wasRunning && coreRunning;
}

function showSettingsError(message) {
  $("#settings-error").hidden = !message;
  $("#settings-error").textContent = message ? `설정 파일 오류: ${message} (이전 설정으로 동작 중)` : "";
}

async function refreshStatus() {
  try {
    const core = await invoke("status");
    // 코어가 새로 켜졌고 화면에 저장하지 않은 변경이 없으면 코어의 설정으로 다시 읽는다.
    if (renderCore(core) && !isDirty()) await load();
  } catch {}
}

$("#start-core").addEventListener("click", async () => {
  $("#start-core").disabled = true;
  try {
    await invoke("start_core");
    $("#core-title").textContent = "Glide를 시작하는 중…";
    // 코어가 파이프를 열 때까지 잠시 기다렸다가 상태를 다시 읽는다.
    for (let attempt = 0; attempt < 10 && !coreRunning; attempt++) {
      await new Promise(resolve => setTimeout(resolve, 500));
      await refreshStatus();
    }
    if (!coreRunning) $("#core-detail").textContent = "Glide가 시작되지 않았습니다. 알림 영역을 확인하거나 다시 시도해 주세요.";
  } catch (error) {
    $("#core-detail").textContent = String(error);
  } finally {
    $("#start-core").disabled = false;
  }
});

// ── 제스처 녹화 ──
function clearRecord() {
  recording = undefined;
  recorded = "";
  recordTrail.clear();
  $("#record-placeholder").hidden = false;
  showRecordedSequence("");
  $("#recorded").textContent = "아직 그리지 않았어요";
  $("#record-error").textContent = "";
  $("#use-record").disabled = true;
}

function openRecorder(index) {
  editingIndex = index;
  draftRule = index === undefined ? { action: "" } : structuredClone(settings.rules[index]);
  clearRecord();
  $("#record-binding").replaceChildren(bindingControls(draftRule, validateRecord, "등록할 제스처"));
  $("#recorder").showModal();
}

function validateRecord() {
  const duplicate = settings.rules.some((rule, index) => rule.gesture === recorded && index !== editingIndex);
  let error = duplicate ? "이미 등록된 제스처입니다. 다른 방향으로 그려 주세요." : "";
  const missingAction = !draftRule.action;
  const missingShortcut = draftRule.action === "keyboard" && !draftRule.shortcut.trim();
  if (missingAction) error ||= "실행할 동작을 선택해 주세요.";
  else if (draftRule.action === "keyboard" && !missingShortcut) {
    try { GestureKit.parseShortcut(draftRule.shortcut); } catch (caught) { error ||= caught.message; }
  }
  $("#record-error").textContent = error;
  $("#use-record").disabled = !recorded || missingAction || missingShortcut || !!error;
}

$("#draw-area").addEventListener("contextmenu", event => event.preventDefault());
let shownSequence;
function showRecordedSequence(sequence) {
  if (sequence === shownSequence) return;
  shownSequence = sequence;
  $("#record-glyph").replaceChildren(...(sequence ? [glyphElement(sequence, 52)] : []));
  $("#recorded").classList.toggle("arrows", !!sequence);
  if (sequence) $("#recorded").textContent = arrows(sequence);
}

let drawRect;
$("#draw-area").addEventListener("pointerdown", event => {
  if (event.button !== 2 || event.buttons !== 2) return;
  clearRecord();
  drawRect = event.currentTarget.getBoundingClientRect();
  recording = new Gesture(event.clientX, event.clientY, settings.threshold);
  recordingPointer = event.pointerId;
  event.currentTarget.setPointerCapture(event.pointerId);
  recordTrail.color = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || recordTrail.color;
  recordTrail.start(event.clientX - drawRect.left, event.clientY - drawRect.top, drawRect);
  $("#record-placeholder").hidden = true;
  $("#recorded").textContent = "그리는 중…";
  event.preventDefault();
});
$("#draw-area").addEventListener("pointermove", event => {
  if (!recording || event.pointerId !== recordingPointer || !(event.buttons & 2)) return;
  recording.move(event.clientX, event.clientY);
  recordTrail.add(event.clientX - drawRect.left, event.clientY - drawRect.top);
  if (!recording.invalid) showRecordedSequence(recording.sequence);
});
$("#draw-area").addEventListener("pointerup", event => {
  if (!recording || event.pointerId !== recordingPointer || event.button !== 2) return;
  recording.move(event.clientX, event.clientY);
  recorded = recording.invalid ? "" : recording.sequence;
  recording = undefined;
  showRecordedSequence(recorded);
  if (!recorded) $("#recorded").textContent = "조금 더 길고 분명하게 그려 주세요";
  validateRecord();
});
$("#draw-area").addEventListener("pointercancel", clearRecord);
$("#draw-area").addEventListener("lostpointercapture", () => { if (recording) clearRecord(); });
window.addEventListener("blur", () => { if (recording) clearRecord(); });
$("#recorder").addEventListener("close", clearRecord);
$("#cancel-record").addEventListener("click", () => $("#recorder").close());
$("#clear-record").addEventListener("click", clearRecord);
$("#use-record").addEventListener("click", () => {
  if (!recorded || $("#use-record").disabled) return;
  const rule = { ...draftRule, gesture: recorded };
  if (rule.action === "keyboard") rule.shortcut = GestureKit.parseShortcut(rule.shortcut).text;
  if (editingIndex === undefined) settings.rules.push(rule);
  else settings.rules[editingIndex] = rule;
  $("#recorder").close();
  render();
  dirty();
});

// ── 기본 설정 ──
$("#add").addEventListener("click", () => openRecorder());
$("#enabled").addEventListener("change", event => { settings.enabled = event.target.checked; dirty(); });
$("#threshold").addEventListener("input", event => {
  settings.threshold = Number(event.target.value);
  renderThreshold();
  dirty();
});
$("#hold").addEventListener("input", event => {
  settings.windows.holdToPassMs = Number(event.target.value);
  renderHold();
  dirty();
});
$("#auto-pause").addEventListener("change", event => { settings.windows.autoPauseFullscreen = event.target.checked; dirty(); });
$("#exclude-form").addEventListener("submit", event => {
  event.preventDefault();
  const input = $("#exclude-input");
  const name = input.value.trim().toLowerCase();
  let error = "";
  // 코어와 같은 규칙: 경로가 아닌 실행 파일 이름만 받는다.
  if (!name) error = "실행 파일 이름을 적어 주세요. 예: mstsc.exe";
  else if (/[\\/]/.test(name)) error = "경로 없이 실행 파일 이름만 적어 주세요. 예: mstsc.exe";
  else if (!name.endsWith(".exe")) error = "실행 파일 이름은 .exe로 끝납니다. 작업 관리자의 '세부 정보' 탭에서 확인할 수 있습니다.";
  else if (settings.windows.excludedProcesses.includes(name)) error = "이미 목록에 있습니다.";
  else if (settings.windows.excludedProcesses.length >= 256) error = "예외 앱은 최대 256개까지 넣을 수 있습니다.";
  $("#exclude-error").textContent = error;
  if (error) return;
  settings.windows.excludedProcesses.push(name);
  input.value = "";
  renderExcluded();
  dirty();
});
$("#reset").addEventListener("click", () => {
  settings.rules = structuredClone(GestureKit.defaults.rules);
  render();
  dirty();
  status("기본 제스처를 불러왔습니다. 저장하면 적용됩니다.");
});

// ── 백업 ──
function ruleNames(rules) {
  return rules.map(rule => `${arrows(rule.gesture)} ${actions[rule.action]?.label ?? rule.action}`).join(", ");
}

async function exportGestures(extensionCompatible) {
  if (!settings) return;
  try {
    const result = await invoke("export_backup", {
      rules: settings.rules,
      extensionCompatible,
      fileName: `glide-gestures${extensionCompatible ? "-extension" : ""}-${dateStamp()}.json`
    });
    if (!result) return;
    const summary = result.count ? `제스처 ${result.count}개를` : "제스처 목록을";
    const skipped = result.skipped.length ? ` 확장에 없는 동작은 뺐습니다: ${ruleNames(result.skipped)}` : "";
    backupStatus(`${summary} ${result.fileName} 파일로 내보냈습니다.${skipped}`);
  } catch (error) {
    backupStatus(`내보내지 못했습니다. ${error}`, true);
  }
}
$("#export-gestures").addEventListener("click", () => exportGestures(false));
$("#export-extension").addEventListener("click", () => exportGestures(true));
$("#import-gestures").addEventListener("click", async () => {
  if (!settings) return;
  try {
    const result = await invoke("import_backup");
    if (!result) return;
    if (isDirty() && !confirm("저장하지 않은 변경사항이 있습니다. 불러온 제스처로 현재 목록을 바꿀까요?")) return;
    settings.rules = result.rules;
    render();
    dirty();
    const summary = result.rules.length ? `제스처 ${result.rules.length}개를 불러왔습니다.` : "제스처가 없는 파일을 불러왔습니다.";
    const skipped = result.skipped.length ? ` Windows에서 실행할 수 없어 뺀 제스처: ${ruleNames(result.skipped)}.` : "";
    backupStatus(`${summary}${skipped} 변경사항 저장을 눌러 적용하세요.`);
  } catch (error) {
    backupStatus(`불러오지 못했습니다. ${error}`, true);
  }
});

// ── 저장 ──
$("#save").addEventListener("click", async () => {
  try {
    // 화면에서 먼저 규칙을 검증해 오류를 빨리 보여 준다. 최종 검증은 코어가 다시 한다.
    const rules = GestureKit.validateRules(settings.rules);
    $("#save").disabled = true;
    await invoke("save", { settings: { schemaVersion: 1, enabled: settings.enabled, threshold: settings.threshold, rules, windows: settings.windows } });
    $("#save").dataset.dirty = "false";
    clearBackupStatus();
    showSettingsError("");
    status("저장했습니다. 바로 적용됩니다.");
  } catch (error) {
    $("#save").disabled = !coreRunning;
    status(error.message || String(error), true);
  }
});
window.addEventListener("beforeunload", event => {
  if (isDirty()) { event.preventDefault(); event.returnValue = ""; }
});

async function load() {
  const data = await invoke("load");
  applyCatalog(data.catalog);
  settings = data.settings;
  renderCore(data.core);
  showSettingsError(data.settingsError);
  $("#save").dataset.dirty = "false";
  $("#save").disabled = true;
  render();
  status(coreRunning ? "" : "Glide가 꺼져 있어 설정을 보기만 할 수 있습니다.", false);
}

load().catch(error => {
  status(`설정을 읽지 못했습니다. ${error}`, true);
});
// 상태 칸(훅 처리 시간·실행 여부)을 주기적으로 갱신한다. 창이 가려져 있으면 쉰다.
setInterval(() => { if (document.visibilityState === "visible" && settings) refreshStatus(); }, 3000);
