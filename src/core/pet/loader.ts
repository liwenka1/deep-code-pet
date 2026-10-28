// 素材加载与校验：pet.json（5 个字段）+ 精灵图尺寸。
// 纯逻辑；文件读取由 Tauri 命令或浏览器的 File API 提供。

import { COLS, FRAME_H, FRAME_W, atlasSize } from "./model";

export const MIN_SIDE = 256; // 素材规范的下限
export const REQUIRED = ["id", "displayName", "spritesheetPath"];

/**
 * @param {string} text
 * @returns {{ pet: {id:string, displayName:string, description:string, version:number, spritesheetPath:string}, warnings: string[] }}
 */
export function parsePetJson(text: string) {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (error) {
    throw new Error(`pet.json 不是合法 JSON：${error instanceof Error ? error.message : String(error)}`);
  }
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw new Error("pet.json 顶层必须是对象");
  }
  const record = raw as Record<string, unknown>;
  const missing = REQUIRED.filter((key) => typeof record[key] !== "string" || record[key].trim() === "");
  if (missing.length > 0) {
    throw new Error(`pet.json 缺字段：${missing.join(", ")}`);
  }

  const warnings: string[] = [];
  let version = 1;
  if (record.spriteVersionNumber != null) {
    const parsed = Number(record.spriteVersionNumber);
    if (parsed === 1 || parsed === 2) version = parsed;
    else warnings.push(`spriteVersionNumber=${record.spriteVersionNumber} 不认识，按 v1 处理`);
  }

  return {
    pet: {
      id: record.id as string,
      displayName: record.displayName as string,
      description: typeof record.description === "string" ? record.description : "",
      version,
      spritesheetPath: record.spritesheetPath as string,
    },
    warnings,
  };
}

/**
 * @param {{width:number, height:number}} size
 * @param {number} version
 */
export function validateAtlas(size: { width: number; height: number }, version = 1) {
  const warnings: string[] = [];
  const expected = atlasSize(version);

  if (size.width < MIN_SIDE || size.height < MIN_SIDE) {
    throw new Error(`精灵图太小：${size.width}×${size.height}（下限 ${MIN_SIDE}）`);
  }

  const cols = size.width / FRAME_W;
  const rows = size.height / FRAME_H;
  if (!Number.isInteger(cols)) warnings.push(`${size.width}px 宽不是 ${FRAME_W} 的整数倍，帧会错位`);
  if (!Number.isInteger(rows)) warnings.push(`${size.height}px 高不是 ${FRAME_H} 的整数倍，帧会错位`);
  if (Number.isInteger(cols) && cols !== COLS) warnings.push(`列数是 ${cols}，格式固定 ${COLS} 列`);
  if (Number.isInteger(rows) && rows < expected.rows) {
    warnings.push(`声明 v${version}（${expected.rows} 行）但图只有 ${rows} 行，缺的状态会空白`);
  }
  if (Number.isInteger(rows) && rows > expected.rows) {
    warnings.push(`图有 ${rows} 行，多出的 ${rows - expected.rows} 行按约定留给消费方`);
  }

  return {
    ok: warnings.length === 0,
    warnings,
    geometry: {
      cols: Number.isInteger(cols) ? cols : COLS,
      rows: Number.isInteger(rows) ? rows : expected.rows,
    },
  };
}
