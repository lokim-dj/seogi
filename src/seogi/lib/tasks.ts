// How a task line reads under its group. A group titled "우리 숙제" already
// says whose task it is, so a line does not say it again: neither in the
// owner beside it nor as the subject its sentence starts with. Only what
// is drawn changes; the minutes as saved keep every word.
//
// The same rules are in src-tauri/src/seogi/render.rs for 회의록.md, and
// both are tested against tasks.cases.json; keep the two alike.

/** Whose tasks a group holds, when its title says so. */
export type Side = "us" | "customer" | "them";

const SIDES: Side[] = ["us", "customer", "them"];

/** The words that name a side in a group's title or as an owner. */
const SIDE_WORDS: Record<Side, string> = {
  us: "우리|저희",
  customer: "고객",
  them: "상대\\s*업체|상대방|상대|업체",
};

/** The words that may be taken off as a sentence's subject. A bare "업체"
 *  is left: "업체는 두 곳으로 좁혀" speaks of vendors, not to them. */
const SUBJECT_WORDS: Record<Side, string> = {
  us: "우리|저희",
  customer: "고객",
  them: "상대\\s*업체|상대방|상대",
};

/** Titles that may follow a name: "김서윤 실장이". */
const TITLES =
  "실장|팀장|소장|대표|이사|부장|차장|과장|대리|주임|매니저|디자이너|선생";

/** The particles a subject ends with, longest first. */
const PARTICLES = "에서는|에서|께서는|께서|이|가|은|는";

/** What, right after the subject, says the subject is not the one who
 *  does the task: "우리가 아니라 …", "저희는 빼고 …". */
const NOT_THE_DOER = /^(?:아니라|아닌|빼고|물론|말고)/;

/** Endings of a word that describes the next one or sets a condition:
 *  "고객이 원하는 타일", "업체가 정해지면". Whose word it is stays. */
const DESCRIBES = /(?:는|은|던|진|른|낸|든|쓴|한|된|뀐|면)$/;
/** Words with such an ending that only tell the time or name a thing. */
const DESCRIBES_NOT = new Set([
  "사진",
  "기한",
  "이번",
  "지난",
  "한번",
  "도면",
  "화면",
  "벽면",
  "측면",
  "전면",
  "정면",
  "평면",
  "단면",
  "표면",
]);

/** Endings of a word that is the subject of a clause: "관리실이". */
const ANOTHER_SUBJECT = /(?:이|가|께서|은|는)$/;
/** Words with such an ending that are not subjects. */
const ANOTHER_SUBJECT_NOT = new Set([
  "같이",
  "깊이",
  "높이",
  "넓이",
  "길이",
  "많이",
  "깨끗이",
  "일찍이",
  "사이",
  "차이",
  "종이",
  "추가",
  "단가",
  "원가",
  "정가",
]);

const SPEAKER_LABEL = /^화자/;

/** Whether an owner is a speaker label ("화자 2", "화자 A (디자이너)")
 *  rather than someone's name. */
export function isSpeakerLabel(owner: string): boolean {
  return SPEAKER_LABEL.test(owner.trim());
}

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The side a group of tasks belongs to, read from its title alone:
 *  "우리 숙제", "고객 숙제", "업체 할 일". "업체에 요청할 일" or "고객 응대
 *  개선" is not a side's group. */
export function sideOf(section: { title: string }): Side | null {
  const title = section.title.trim();
  return (
    SIDES.find((side) =>
      new RegExp(
        `^(?:${SIDE_WORDS[side]})(?:\\s*(?:쪽|측))?\\s*(?:숙제|할\\s*일)$`,
      ).test(title),
    ) ?? null
  );
}

/** The side an owner is, when the owner is a side and not a person:
 *  "우리(상담 담당)", "고객님", "상대 업체" — but not "고객 김서윤". */
function sideOwner(owner: string): Side | null {
  return (
    SIDES.find((side) =>
      new RegExp(
        `^(?:${SIDE_WORDS[side]})(?:\\s*님)?(?:\\s*(?:쪽|측))?\\s*(?:\\(.*\\))?$`,
      ).test(owner),
    ) ?? null
  );
}

function sideSubject(side: Side): string {
  return `(?:${SUBJECT_WORDS[side]})(?:\\s*님)?(?:\\s*(?:쪽|측))?`;
}

/** Whether `owner` is one of the people of the meeting. */
function listed(owner: string, names: string[]): boolean {
  return names
    .map((name) => name.trim())
    .filter((name) => name && !isSpeakerLabel(name))
    .some(
      (name) =>
        name === owner ||
        owner.startsWith(`${name} `) ||
        name.startsWith(`${owner} `),
    );
}

/** A person's name as a subject. A name shorter than three letters must
 *  come with a title or 님, so that an owner that is also a word ("유리",
 *  "이사") is not taken off a sentence about the thing. */
function nameSubject(owner: string): string {
  const words = owner.split(/\s+/).filter(Boolean);
  const forms = [owner, words.join("")];
  if (words.length > 1 && words[0].length >= 2) forms.push(words[0]);
  const unique = [...new Set(forms)].sort((a, b) => b.length - a.length);
  const title = `(?:\\s*(?:${TITLES}))`;
  return `(?:${unique
    .map((form) => {
      const name = escape(form).replace(/ /g, "\\s*");
      return form.length >= 3
        ? `${name}${title}?(?:\\s*님)?`
        : `${name}(?:${title}(?:\\s*님)?|\\s*님)`;
    })
    .join("|")})`;
}

/** Takes the subject `pattern` describes off the front of `text`, unless
 *  that would change what the sentence says. */
function dropSubject(text: string, pattern: string): string {
  const match = new RegExp(`^(?:${pattern})(?:${PARTICLES})[\\s,]+`).exec(text);
  if (!match) return text;
  const rest = text.slice(match[0].length).trim();
  if (!rest || /[,，]/.test(rest)) return text;
  const words = rest.split(/\s+/);
  const bare = (word: string) => word.replace(/[.!?。]+$/, "");
  const first = bare(words[0]);
  if (NOT_THE_DOER.test(first)) return text;
  if (DESCRIBES.test(first) && !DESCRIBES_NOT.has(first)) return text;
  if (
    words
      .map(bare)
      .some(
        (word) =>
          word.length >= 2 &&
          ANOTHER_SUBJECT.test(word) &&
          !ANOTHER_SUBJECT_NOT.has(word),
      )
  )
    return text;
  return rest;
}

/** One task line as drawn. `who` is null when the line names no one
 *  (the group says the side, or nobody is known), otherwise the person
 *  or side to show beside it. `unowned` is true when nobody is known, so a
 *  small "담당 미정" is shown in place of a name. `names` are the people of
 *  the meeting; only theirs is taken off as a subject. */
export function taskLine(
  section: { title: string },
  entry: { owner: string; text: string },
  names: string[],
): { who: string | null; text: string; unowned: boolean } {
  const side = sideOf(section);
  const raw = entry.owner.trim();
  const owner = isSpeakerLabel(raw) ? "" : raw;
  const ownerSide = owner ? sideOwner(owner) : null;

  let text = entry.text.trim();
  if (side) text = dropSubject(text, sideSubject(side));

  if (!owner) return { who: null, text, unowned: true };
  if (side && ownerSide === side) return { who: null, text, unowned: false };
  if (ownerSide) text = dropSubject(text, sideSubject(ownerSide));
  else if (listed(owner, names)) text = dropSubject(text, nameSubject(owner));
  return { who: owner, text, unowned: false };
}

/** Whether a group's title is needed to say whose tasks they are. */
export function titleNeeded(
  section: { title: string },
  groups: number,
): boolean {
  return groups > 1 || sideOf(section) !== null;
}
