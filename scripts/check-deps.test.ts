import { expect, test } from "bun:test";
import { cvss3Score } from "./check-deps";

// Expected scores from the FIRST CVSS 3.1 calculator.
test("CVSS 3.x base scores", () => {
  expect(cvss3Score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H")).toBe(9.8);
  expect(cvss3Score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H")).toBe(7.5);
  expect(cvss3Score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H")).toBe(10);
  expect(cvss3Score("CVSS:3.1/AV:L/AC:H/PR:N/UI:N/S:C/C:L/I:N/A:N")).toBe(3.2);
  expect(cvss3Score("CVSS:3.1/AV:L/AC:L/PR:L/UI:R/S:U/C:H/I:H/A:H")).toBe(7.3);
  expect(cvss3Score("CVSS:3.0/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:N")).toBe(0);
});

test("anything but CVSS 3.x has no score", () => {
  expect(
    cvss3Score(
      "CVSS:4.0/AV:L/AC:L/AT:N/PR:N/UI:P/VC:H/VI:N/VA:N/SC:N/SI:N/SA:N",
    ),
  ).toBeUndefined();
  expect(cvss3Score("")).toBeUndefined();
  expect(
    cvss3Score("CVSS:3.1/AV:X/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"),
  ).toBeUndefined();
});
