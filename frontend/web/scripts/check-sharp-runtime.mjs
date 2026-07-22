import { createRequire } from "node:module";

// Resolve from Next's package boundary so this exercises the optional Sharp
// dependency exactly as the production image optimizer loads it under pnpm's
// strict node_modules layout.
const requireFromNext = createRequire(import.meta.resolve("next/package.json"));
const sharp = requireFromNext("sharp");
const [major, minor] = sharp.versions.sharp.split(".").map(Number);

if (major !== 0 || minor < 35) {
  throw new Error(`Sharp security floor is 0.35.x; found ${sharp.versions.sharp}`);
}

const { info } = await sharp({
  create: {
    width: 1,
    height: 1,
    channels: 4,
    background: { r: 255, g: 0, b: 0, alpha: 1 },
  },
})
  .resize(2, 2)
  .png()
  .toBuffer({ resolveWithObject: true });

if (info.width !== 2 || info.height !== 2 || info.format !== "png") {
  throw new Error(`Sharp runtime smoke returned ${info.width}x${info.height} ${info.format}`);
}

console.log(`Sharp runtime: ${sharp.versions.sharp}, ${info.width}x${info.height} ${info.format}`);
