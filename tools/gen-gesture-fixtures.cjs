#!/usr/bin/env node
// 크롬 확장(Glide)의 shared.js를 기준 구현으로 삼아 방향 인식 픽스처를 만든다.
// 같은 좌표 입력에 대해 확장(JS)과 Windows 앱(Rust)이 같은 결과를 내는지
// `cargo test`가 이 파일로 검증한다.
//
// 사용법: node tools/gen-gesture-fixtures.cjs <확장 저장소의 shared.js 경로>
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const sharedPath = process.argv[2];
if (!sharedPath) {
  console.error("사용법: node tools/gen-gesture-fixtures.cjs <shared.js 경로>");
  process.exit(2);
}
const source = fs.readFileSync(sharedPath, "utf8");
const kit = vm.runInNewContext(source + "; GestureKit", { structuredClone });

// 결과가 실행마다 바뀌지 않도록 시드를 고정한 의사 난수(mulberry32).
function random(seed) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// 방향 글자열을 따라 일정 간격으로 점을 찍는다. jitter는 진행 방향의 수직 떨림(px)이다.
function stroke(start, directions, { length = 120, step = 6, jitter = 0, seed = 1 } = {}) {
  const next = random(seed);
  const vectors = { L: [-1, 0], R: [1, 0], U: [0, -1], D: [0, 1] };
  const points = [];
  let [x, y] = start;
  for (const direction of directions) {
    const [vx, vy] = vectors[direction];
    for (let travelled = step; travelled <= length; travelled += step) {
      const wobble = Math.round((next() * 2 - 1) * jitter);
      x += vx * step;
      y += vy * step;
      points.push([x + (vx === 0 ? wobble : 0), y + (vy === 0 ? wobble : 0)]);
    }
  }
  return points;
}

const cases = [
  // shared.js 단위 테스트(tests/core.test.cjs)와 같은 입력
  {
    name: "손떨림은 무시하고 직선 이동을 합친다",
    start: [200, 200],
    points: [
      ...Array.from({ length: 15 }, (_, i) => [200 + (i % 5), 200 - (i % 6)]),
      ...Array.from({ length: 14 }, (_, i) => { const x = 180 - i * 10; return [x, 200 + (x % 3)]; })
    ]
  },
  { name: "꺾임·역방향·빠른 이동", start: [200, 200], points: [[200, 300], [320, 300], [320, 200], [200, 200], [200, 300]] },
  { name: "대각선에서는 분명한 방향을 기다린다", start: [0, 0], points: [[30, 30], [70, 35]] },
  { name: "민감한 임계값 12", start: [0, 0], threshold: 12, points: [[20, 0]] },
  { name: "둔한 임계값 48", start: [0, 0], threshold: 48, points: [[20, 0]] },
  // 경계값: 임계값과 정확히 같은 거리(22)는 이동으로 본다
  { name: "임계값과 같은 거리", start: [0, 0], points: [[22, 0]] },
  { name: "임계값보다 짧은 거리", start: [0, 0], points: [[21, 0]] },
  { name: "3-4-5 직각삼각형 거리", start: [0, 0], threshold: 20, points: [[12, 16]] },
  // 기본 제스처 10종(확장 defaults.rules)
  ...["L", "R", "D", "DR", "UD", "DU", "UL", "UR", "U", "DL"].map((sequence, index) => ({
    name: `기본 제스처 ${sequence}`, start: [500, 500], points: stroke([500, 500], sequence, { jitter: 4, seed: index + 1 })
  })),
  // 최대 12획과 초과(무효)
  { name: "12획 지그재그", start: [500, 500], points: stroke([500, 500], "RDRDRDRDRDRD", { length: 60 }) },
  { name: "13획은 무효", start: [500, 500], points: stroke([500, 500], "RDRDRDRDRDRDR", { length: 60 }) },
  // 떨림이 큰 손, 짧은 획, 고배율 모니터처럼 큰 좌표
  ...[1, 2, 3, 4, 5].map(seed => ({
    name: `떨림 큰 L자 #${seed}`, start: [300, 300], points: stroke([300, 300], "DR", { jitter: 12, seed: 100 + seed })
  })),
  { name: "짧은 획 연속", start: [0, 0], points: stroke([0, 0], "RULD", { length: 30, step: 3 }) },
  { name: "4K 좌표", start: [3600, 2000], threshold: 33, points: stroke([3600, 2000], "LU", { length: 300, step: 15, jitter: 6, seed: 7 }) },
  { name: "음수 좌표(왼쪽 보조 모니터)", start: [-1500, 300], points: stroke([-1500, 300], "UR", { jitter: 3, seed: 9 }) }
];

const fixtures = cases.map(item => {
  const threshold = item.threshold ?? 22;
  const gesture = new kit.Gesture(item.start[0], item.start[1], threshold);
  for (const [x, y] of item.points) gesture.move(x, y);
  return {
    name: item.name,
    threshold,
    start: item.start,
    points: item.points,
    expected: { sequence: gesture.sequence, moved: gesture.moved, invalid: gesture.invalid }
  };
});

const output = path.join(__dirname, "..", "fixtures", "gestures.json");
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, JSON.stringify({
  generatedFrom: "Glide 확장 shared.js (Gesture 클래스)",
  cases: fixtures
}, null, 2) + "\n");
console.log(`${fixtures.length}개 픽스처를 ${path.relative(process.cwd(), output)}에 썼습니다.`);

// 단축키 파서(parseShortcut) 픽스처: 정상 입력은 표기·키 코드·보조 키를, 잘못된 입력은 오류 문구를 기록한다.
const shortcutInputs = [
  "Ctrl+T", "ctrl + shift + t", "Alt+Left", "alt+right", "Shift+F5", "F11", "f24", "F25", "F0",
  "Ctrl++", "Ctrl + +", "Ctrl+=", "Ctrl+-", "Ctrl+0", "Shift+1", "Cmd+D", "command+option+k", "Meta+[",
  "Space", "spacebar", "Enter", "return", "Esc", "Tab", "Backspace", "Del", "Insert", "Home", "End",
  "PgUp", "pgdn", "Up", "Down", "Ctrl+Home", "Ctrl+End", "Shift+/", "Ctrl+?", "Ctrl+Shift+`", "Ctrl+~",
  "Ctrl+;", "Ctrl+:", "Alt+'", "Alt+\"", "Ctrl+,", "Ctrl+<", "Ctrl+.", "Ctrl+\\", "Ctrl+|", "Ctrl+]",
  "Ctrl+}", "Ctrl+_", "", "   ", "Ctrl+", "+T", "Ctrl+Ctrl+T", "Hyper+T", "Ctrl+Shift", "Ctrl+Alt+Shift+Cmd+Z",
  "Ctrl+한", "x".repeat(81), "Ctrl+ArrowLeft", "ArrowDown", "Option+Shift+F12"
];
const shortcuts = shortcutInputs.map(input => {
  try {
    const parsed = kit.parseShortcut(input);
    return {
      input,
      ok: {
        text: parsed.text, keyCode: parsed.keyCode,
        ctrl: parsed.ctrlKey, alt: parsed.altKey, shift: parsed.shiftKey, meta: parsed.metaKey
      }
    };
  } catch (error) {
    return { input, error: error.message };
  }
});
const shortcutOutput = path.join(__dirname, "..", "fixtures", "shortcuts.json");
fs.writeFileSync(shortcutOutput, JSON.stringify({
  generatedFrom: "Glide 확장 shared.js (parseShortcut)",
  cases: shortcuts
}, null, 2) + "\n");
console.log(`${shortcuts.length}개 픽스처를 ${path.relative(process.cwd(), shortcutOutput)}에 썼습니다.`);
