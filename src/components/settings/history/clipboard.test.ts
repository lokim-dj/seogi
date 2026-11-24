// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

import assert from "node:assert/strict";
import { copyToClipboard } from "./clipboard";

const successfulClipboard = {
  writeText: async () => {},
};

const failedClipboard = {
  writeText: async () => {
    throw new Error("clipboard unavailable");
  },
};

assert.equal(await copyToClipboard("copied text", successfulClipboard), true);
assert.equal(await copyToClipboard("copied text", failedClipboard), false);

console.log("clipboard: all assertions passed");
