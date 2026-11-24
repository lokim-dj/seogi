// Seogi's texts live beside its screens, in Korean and English, as one
// namespace added to the app's i18next instance. Other languages fall back
// to English.

import i18n from "@/i18n";
import en from "./en";
import ko from "./ko";

export const NS = "seogi";

i18n.addResourceBundle("ko", NS, ko, true, true);
i18n.addResourceBundle("en", NS, en, true, true);

// The app settles its language a moment after it starts. A Korean system
// should not see a flash of English before that.
if (
  navigator.language.toLowerCase().startsWith("ko") &&
  i18n.language !== "ko"
) {
  void i18n.changeLanguage("ko");
}

export default i18n;
