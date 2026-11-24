// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

import React from "react";

/** An inline, low-emphasis action within a sentence. */
export const TextLink: React.FC<
  React.ButtonHTMLAttributes<HTMLButtonElement>
> = ({ className = "", type = "button", ...props }) => (
  <button
    type={type}
    className={`py-[2px] text-small text-ink-2 underline decoration-line underline-offset-[3px] cursor-pointer hover:text-text hover:decoration-current ${className}`}
    {...props}
  />
);
