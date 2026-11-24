// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

import React from "react";
import seogiIcon from "@/assets/seogi-icon.png";

interface MascotProps {
  /** Height in px. */
  size: number;
  /** Wave once on arrival, or bounce (re-key it to play again). */
  animation?: "wave" | "slap" | null;
}

/** Seogi's mark, optionally animated. */
export const Mascot: React.FC<MascotProps> = ({ size, animation = null }) => (
  <span
    style={{ width: size, height: size }}
    className={`relative block origin-[50%_90%] select-none ${animation === "wave" ? "animate-wave" : animation === "slap" ? "animate-slap" : ""}`}
  >
    <img
      src={seogiIcon}
      alt=""
      draggable={false}
      className="absolute inset-0 h-full w-full"
    />
  </span>
);
