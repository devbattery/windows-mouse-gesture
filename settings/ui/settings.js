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
  renderSuggested();
  renderHistory();
}

/// 예외 앱을 화면 목록에 넣는다(저장은 저장 바로). 문제가 있으면 안내 문구를 돌려준다.
function addExclusion(raw) {
  const name = String(raw).trim().toLowerCase();
  // 코어와 같은 규칙: 경로가 아닌 실행 파일 이름만 받는다.
  if (!name) return "실행 파일 이름을 적어 주세요. 예: mstsc.exe";
  if (/[\\/]/.test(name)) return "경로 없이 실행 파일 이름만 적어 주세요. 예: mstsc.exe";
  if (!name.endsWith(".exe")) return "실행 파일 이름은 .exe로 끝납니다. 작업 관리자의 '세부 정보' 탭에서 확인할 수 있습니다.";
  if (settings.windows.excludedProcesses.includes(name)) return "이미 목록에 있습니다.";
  if (settings.windows.excludedProcesses.length >= 256) return "예외 앱은 최대 256개까지 넣을 수 있습니다.";
  settings.windows.excludedProcesses.push(name);
  renderExcluded();
  dirty();
  return "";
}

let suggestedExclusions = [];
function renderSuggested() {
  const list = $("#suggested");
  list.replaceChildren(...suggestedExclusions.map(({ name, reason }) => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    const added = settings.windows.excludedProcesses.includes(name);
    button.disabled = added;
    const code = document.createElement("code");
    code.textContent = name;
    button.append(added ? "✓ " : "+ ", code, ` · ${reason}`);
    button.setAttribute("aria-label", added ? `${name}(${reason}) 이미 예외에 있음` : `${name}(${reason}) 예외에 추가`);
    button.addEventListener("click", () => {
      $("#exclude-error").textContent = addExclusion(name);
    });
    item.append(button);
    return item;
  }));
  list.parentElement.hidden = suggestedExclusions.length === 0;
}

function render() {
  $("#enabled").checked = settings.enabled;
  $("#threshold").value = settings.threshold;
  renderThreshold();
  $("#hold").value = settings.windows.holdToPassMs;
  renderHold();
  $("#auto-pause").checked = settings.windows.autoPauseFullscreen;
  $("#allow-injected").checked = !!settings.windows.allowInjectedInput;
  // 이전 코어가 보낸 설정에는 값이 없을 수 있다. 없으면 기본값(켜짐)으로 본다.
  $("#check-updates").checked = settings.windows.checkUpdates !== false;
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
    // 코어가 확인한 새 버전(지금보다 새 버전일 때만 값이 있다). 받기는 "새 버전 확인" 버튼으로 연다.
    const latest = s.update?.latest;
    $("#about-update").hidden = !latest;
    $("#about-update").textContent = latest ? `새 버전 ${latest}이(가) 있습니다. "새 버전 확인"을 눌러 받아 주세요.` : "";
    const p99 = s.hookCalls === 0 ? "기록 없음" : s.hookOver10ms ? "10ms 이상" : `${s.hookP99Micros}µs 미만`;
    facts.push([`훅 처리 시간 p99: ${p99}`, s.hookOver10ms ? "warn" : ""]);
    if (s.reinstalls) facts.push([`훅 재설치 ${s.reinstalls}회`, ""]);
  } else {
    $("#about-update").hidden = true;
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
  renderPress();
  dirty();
});
$("#auto-pause").addEventListener("change", event => { settings.windows.autoPauseFullscreen = event.target.checked; dirty(); });
$("#allow-injected").addEventListener("change", event => { settings.windows.allowInjectedInput = event.target.checked; dirty(); });
$("#check-updates").addEventListener("change", event => { settings.windows.checkUpdates = event.target.checked; dirty(); });
$("#exclude-form").addEventListener("submit", event => {
  event.preventDefault();
  const input = $("#exclude-input");
  const error = addExclusion(input.value);
  $("#exclude-error").textContent = error;
  if (!error) input.value = "";
});

// ── 일상 사용 점검 ──
let diagnostics;
let historyKey = "";
const kindText = {
  executed: ["제스처 실행", "ok"],
  failed: ["실행하지 못함", "warn"],
  unmatched: ["등록되지 않은 제스처", ""],
  notGesture: ["방향 없음", ""],
  click: ["단순 우클릭", ""],
  hold: ["길게 눌러 원래 우클릭", ""],
  excluded: ["예외 앱이라 원래 우클릭", ""],
  otherButton: ["다른 버튼과 함께 눌림", ""],
  cancelled: ["취소됨", "warn"]
};

function checkupStatus(text, error = false) {
  $("#checkup-status").textContent = text;
  $("#checkup-status").dataset.error = error;
}

function entryDetail(entry) {
  if (!entry.detail) return "";
  if (entry.kind === "executed") return actions[entry.detail]?.label ?? entry.detail;
  if (entry.kind === "unmatched" || entry.kind === "notGesture") return arrows(entry.detail);
  return entry.detail;
}

function timeText(at) {
  return new Date(at).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function msText(value) {
  if (value === null || value === undefined) return "기록 없음";
  return value < 0 ? "2초 이상" : `${value}ms 이하`;
}

function renderPress() {
  const press = diagnostics?.press;
  const apply = $("#apply-hold");
  if (!press) {
    $("#press-stats").textContent = "Glide가 실행 중일 때 기록됩니다.";
    apply.hidden = true;
    return;
  }
  const parts = [
    `제스처 시작까지 걸린 시간(95%): ${msText(press.gestureStartP95)} · ${press.gestureStartCount}회`,
    `단순 우클릭 누름 시간(95%): ${msText(press.clickP95)} · ${press.clickCount}회`
  ];
  let advice;
  if (press.recommendedHoldMs) {
    advice = press.recommendedHoldMs === settings.windows.holdToPassMs
      ? `지금 값(${(press.recommendedHoldMs / 1000).toFixed(2)}초)이 추천값과 같습니다.`
      : `추천 ${(press.recommendedHoldMs / 1000).toFixed(2)}초 · 화면의 값 ${(settings.windows.holdToPassMs / 1000).toFixed(2)}초`;
  } else {
    advice = `제스처를 ${press.minSamples}번 이상 쓰면 추천값을 계산합니다.`;
  }
  $("#press-stats").textContent = `${parts.join(" / ")}. ${advice}`;
  apply.hidden = !press.recommendedHoldMs || press.recommendedHoldMs === settings.windows.holdToPassMs;
}

function renderHistory() {
  const history = diagnostics?.history;
  const entries = history?.entries ?? [];
  const counts = history?.counts ?? {};
  const total = Object.values(counts).reduce((sum, count) => sum + count, 0);
  $("#history-counts").textContent = total ? `${total}회 중 실행 ${counts.executed ?? 0}회` : "";
  $("#history-empty").hidden = entries.length !== 0;
  $("#history-empty").textContent = diagnostics ? "아직 기록이 없습니다. 아무 앱에서나 우클릭해 보세요." : "Glide가 실행 중일 때 기록됩니다.";
  // 같은 내용이면 다시 그리지 않는다(버튼 초점 유지). 예외 목록이 바뀌면 버튼 상태가 달라지므로 함께 비교한다.
  const key = JSON.stringify([entries, settings?.windows.excludedProcesses]);
  if (key === historyKey) return;
  historyKey = key;
  $("#history").replaceChildren(...entries.map(entry => {
    const row = document.createElement("tr");
    const time = row.insertCell();
    time.className = "time";
    time.textContent = timeText(entry.at);
    const app = row.insertCell();
    app.className = "app";
    app.textContent = entry.app || "알 수 없음";
    const result = row.insertCell();
    const [label, tone] = kindText[entry.kind] ?? [entry.kind, ""];
    const detail = entryDetail(entry);
    result.textContent = detail ? `${label} · ${detail}` : label;
    if (tone) result.dataset.tone = tone;
    const action = row.insertCell();
    action.className = "action";
    if (entry.app) {
      const name = entry.app.toLowerCase();
      const excluded = settings.windows.excludedProcesses.includes(name);
      const button = document.createElement("button");
      button.type = "button";
      button.className = "secondary";
      button.textContent = excluded ? "예외에 있음" : "예외에 추가";
      button.disabled = excluded;
      button.setAttribute("aria-label", excluded ? `${name} 이미 예외에 있음` : `${name} 예외에 추가`);
      button.addEventListener("click", () => {
        const error = addExclusion(name);
        checkupStatus(error || `${name}을(를) 예외 목록에 넣었습니다. 변경사항 저장을 눌러 적용하세요.`, !!error);
      });
      action.append(button);
    }
    return row;
  }));
}

async function refreshCheckup() {
  if (!coreRunning) {
    diagnostics = undefined;
  } else {
    try {
      diagnostics = await invoke("diagnostics");
      // 이전 조회 오류 안내는 지운다.
      if ($("#checkup-status").dataset.error === "true") checkupStatus("");
    } catch (error) {
      diagnostics = undefined;
      checkupStatus(String(error), true);
    }
  }
  renderPress();
  renderHistory();
}

/// 문제를 알릴 때 붙여 넣을 진단 정보. 좌표·창 제목은 없고, 앱 이름은 사용자가 고를 때만 넣는다.
function diagnosticsText(withApps) {
  const s = diagnostics.system;
  const p = diagnostics.press;
  const counts = diagnostics.history.counts;
  const lines = [
    `Glide ${s.version} · ${s.os}`,
    `관리자 권한 창 지원: ${s.uiAccess ? "켜짐" : "꺼짐"} · 실행 시간 ${Math.round(s.uptimeSeconds / 60)}분`,
    `모니터: ${s.monitors.map(m => `${m.width}x${m.height} ${m.scalePercent}%${m.primary ? "(주)" : ""}`).join(", ") || "알 수 없음"}`,
    `훅: 호출 ${s.hookCalls}회 · p99 ${s.hookCalls === 0 ? "기록 없음" : s.hookP99Micros === null ? "10ms 이상" : `${s.hookP99Micros}µs 미만`} · 1ms 이상 ${s.hookOver1ms}회 · 10ms 이상 ${s.hookOver10ms}회`,
    `훅 재설치 ${s.reinstalls}회 · 초기화(잠금·절전 등) ${s.resets}회 · 버린 사건 ${s.droppedEvents}개`,
    `설정: 제스처 ${s.ruleCount}개 · 감도 ${s.thresholdPx}px · 길게 누르기 ${s.holdMs}ms · 예외 앱 ${s.excludedCount}개 · 전체 화면 멈춤 ${s.autoPauseFullscreen ? "켬" : "끔"} · 원격 제어 입력 ${s.allowInjectedInput ? "켬" : "끔"} · 새 버전 확인 ${s.checkUpdates === false ? "끔" : "켬"}`,
    `우클릭 결과: ${Object.entries(counts).map(([kind, count]) => `${kindText[kind]?.[0] ?? kind} ${count}`).join(", ")}`,
    `누름 시간: 제스처 시작 p95 ${msText(p.gestureStartP95)}(${p.gestureStartCount}회), 단순 우클릭 p95 ${msText(p.clickP95)}(${p.clickCount}회), 추천 ${p.recommendedHoldMs ? `${p.recommendedHoldMs}ms` : `없음(제스처 ${p.minSamples}회 미만)`}`
  ];
  if (withApps) {
    lines.push("최근 기록:");
    for (const entry of diagnostics.history.entries) {
      const detail = entryDetail(entry);
      lines.push(`  ${timeText(entry.at)} ${entry.app || "알 수 없음"} · ${kindText[entry.kind]?.[0] ?? entry.kind}${detail ? ` · ${detail}` : ""}`);
    }
  }
  return lines.join("\n");
}


$("#refresh-checkup").addEventListener("click", async () => {
  checkupStatus("");
  await refreshCheckup();
  if (coreRunning && diagnostics) checkupStatus("최신 기록을 불러왔습니다.");
});
$("#copy-diagnostics").addEventListener("click", async () => {
  await refreshCheckup();
  if (!diagnostics) {
    checkupStatus("Glide가 실행 중일 때 진단 정보를 복사할 수 있습니다.", true);
    return;
  }
  // WebView2의 클립보드 API는 권한 확인 창을 띄울 수 있어 Rust 쪽에서 복사한다.
  try {
    await invoke("copy_text", { text: diagnosticsText($("#copy-apps").checked) });
    checkupStatus("진단 정보를 클립보드에 복사했습니다.");
  } catch (error) {
    checkupStatus(String(error), true);
  }
});
$("#apply-hold").addEventListener("click", () => {
  const value = diagnostics?.press?.recommendedHoldMs;
  if (!value) return;
  settings.windows.holdToPassMs = value;
  $("#hold").value = value;
  renderHold();
  renderPress();
  dirty();
  checkupStatus(`길게 누르기를 ${(value / 1000).toFixed(2)}초로 바꿨습니다. 변경사항 저장을 눌러 적용하세요.`);
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
    checkupStatus("");
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

// ── Glide 정보 칸 ──
// 버튼은 종류 이름만 보내고, 실제로 여는 주소·파일은 설정 창(Rust)이 정한다.
for (const button of document.querySelectorAll("[data-info]")) {
  button.addEventListener("click", async () => {
    const node = $("#about-status");
    node.hidden = true;
    try {
      await invoke("open_info", { kind: button.dataset.info });
    } catch (error) {
      node.textContent = String(error);
      node.hidden = false;
    }
  });
}

async function load() {
  const data = await invoke("load");
  applyCatalog(data.catalog);
  suggestedExclusions = data.suggestedExclusions ?? [];
  settings = data.settings;
  renderCore(data.core);
  showSettingsError(data.settingsError);
  $("#about-version").textContent = `Glide ${data.version}`;
  $("#save").dataset.dirty = "false";
  $("#save").disabled = true;
  render();
  status(coreRunning ? "" : "Glide가 꺼져 있어 설정을 보기만 할 수 있습니다.", false);
  await refreshCheckup();
}

load().catch(error => {
  status(`설정을 읽지 못했습니다. ${error}`, true);
});
// 상태 칸(훅 처리 시간·실행 여부)과 최근 기록을 주기적으로 갱신한다. 창이 가려져 있으면 쉰다.
setInterval(async () => {
  if (document.visibilityState !== "visible" || !settings) return;
  await refreshStatus();
  await refreshCheckup();
}, 3000);
