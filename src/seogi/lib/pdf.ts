// A one-page PDF holding one JPEG picture that fills the page: enough for
// printing the summary card on A4, with no PDF library in the app. The
// picture goes in as it is (DCTDecode), so nothing is drawn again.

/** A4 in PDF points (1/72 inch). */
export const A4_POINTS = { width: 595.28, height: 841.89 } as const;

/**
 * A PDF of one page `page` points large, filled by the JPEG `jpeg` that is
 * `width` × `height` pixels.
 */
export function jpegPdf(
  jpeg: Uint8Array,
  width: number,
  height: number,
  page: { width: number; height: number } = A4_POINTS,
): Uint8Array {
  const encoder = new TextEncoder();
  const parts: Uint8Array[] = [];
  const offsets: number[] = [];
  let length = 0;
  const put = (part: Uint8Array | string) => {
    const bytes = typeof part === "string" ? encoder.encode(part) : part;
    parts.push(bytes);
    length += bytes.length;
  };
  const object = (body: () => void) => {
    offsets.push(length);
    put(`${offsets.length} 0 obj\n`);
    body();
    put("\nendobj\n");
  };
  const w = page.width.toFixed(2);
  const h = page.height.toFixed(2);
  const draw = `q ${w} 0 0 ${h} 0 0 cm /Im0 Do Q`;

  // The header, with a comment of high bytes so tools take the file as binary.
  put("%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
  object(() => put("<< /Type /Catalog /Pages 2 0 R >>"));
  object(() => put("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"));
  object(() =>
    put(
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${w} ${h}] ` +
        "/Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>",
    ),
  );
  object(() => {
    put(
      `<< /Type /XObject /Subtype /Image /Width ${width} /Height ${height} ` +
        "/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode " +
        `/Length ${jpeg.length} >>\nstream\n`,
    );
    put(jpeg);
    put("\nendstream");
  });
  object(() => put(`<< /Length ${draw.length} >>\nstream\n${draw}\nendstream`));

  const xref = length;
  put(`xref\n0 ${offsets.length + 1}\n0000000000 65535 f \n`);
  for (const offset of offsets) {
    put(`${String(offset).padStart(10, "0")} 00000 n \n`);
  }
  put(
    `trailer\n<< /Size ${offsets.length + 1} /Root 1 0 R >>\n` +
      `startxref\n${xref}\n%%EOF\n`,
  );

  const out = new Uint8Array(length);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}
