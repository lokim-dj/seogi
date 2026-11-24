import React from "react";
import seogiIcon from "@/assets/seogi-icon.png";

// The name is a brand, so it is not translated.
const BRAND_NAME = "서기";

interface SeogiLogoProps {
  /** Height of the mark in px; the wordmark scales with it. */
  size?: number;
  className?: string;
}

/** Seogi's wordmark: the app icon beside the name. */
const SeogiLogo: React.FC<SeogiLogoProps> = ({ size = 28, className }) => (
  <span
    className={`inline-flex items-center gap-2 select-none ${className ?? ""}`}
  >
    <img
      src={seogiIcon}
      width={size * 1.24}
      height={size * 1.24}
      alt=""
      draggable={false}
      className="shrink-0 -m-[3px]"
    />
    <span
      className="font-bold tracking-tight leading-none"
      style={{ fontSize: size * 0.82 }}
    >
      {BRAND_NAME}
    </span>
  </span>
);

export default SeogiLogo;
