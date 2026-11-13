#!/usr/bin/env node
// assets/의 PNG 아이콘(크롬 확장과 같은 그림)을 Windows ICO 파일 하나로 묶는다.
// ICO 항목에 PNG를 그대로 넣는 형식(Windows Vista 이후 지원)이라 변환 도구가 필요 없다.
// 결과(settings/icons/icon.ico)는 설정 창 실행 파일의 아이콘으로 들어간다.
//
// 사용법: node tools/make-icon.cjs
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const root = path.join(__dirname, "..");
const sizes = [16, 32, 48, 128];
const images = sizes.map(size => {
  const data = fs.readFileSync(path.join(root, "assets", `${size}.png`));
  // PNG 머리(IHDR)의 실제 크기가 파일 이름과 같은지 확인한다.
  const width = data.readUInt32BE(16);
  const height = data.readUInt32BE(20);
  if (width !== size || height !== size) throw new Error(`${size}.png 크기가 ${width}x${height}입니다.`);
  return { size, data };
});

// ICONDIR(6바이트) + ICONDIRENTRY(16바이트 × n) + 이미지 데이터
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0); // 예약
header.writeUInt16LE(1, 2); // 1 = 아이콘
header.writeUInt16LE(images.length, 4);
let offset = 6 + 16 * images.length;
const entries = images.map(({ size, data }) => {
  const entry = Buffer.alloc(16);
  entry.writeUInt8(size >= 256 ? 0 : size, 0); // 너비(256은 0)
  entry.writeUInt8(size >= 256 ? 0 : size, 1); // 높이
  entry.writeUInt8(0, 2); // 팔레트 색 수(없음)
  entry.writeUInt8(0, 3); // 예약
  entry.writeUInt16LE(1, 4); // 색 평면
  entry.writeUInt16LE(32, 6); // 픽셀당 비트
  entry.writeUInt32LE(data.length, 8);
  entry.writeUInt32LE(offset, 12);
  offset += data.length;
  return entry;
});

const output = path.join(root, "settings", "icons", "icon.ico");
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, Buffer.concat([header, ...entries, ...images.map(image => image.data)]));
console.log(`${images.length}개 크기(${sizes.join(", ")})를 ${path.relative(process.cwd(), output)}에 묶었습니다.`);
