// Walks Seogi's main flows in the browser preview (src/seogi/preview) and
// checks that each press asks the backend for the right thing. No app, no
// window, no sound:
//
//   bun run dev            # in another terminal
//   node scripts/seogi-flows.mjs

import { chromium } from "@playwright/test";

const base = "http://localhost:1420/src/seogi/preview/index.html?screen=";
const browser = await chromium
  .launch()
  .catch(() => chromium.launch({ channel: "chrome" }));
let failed = 0;

async function flow(name, screen, steps) {
  const page = await browser.newPage({
    viewport: { width: 1280, height: 800 },
    locale: "ko-KR",
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(base + screen, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  const calls = () => page.evaluate(() => window.__seogiCalls);
  const called = async (command, test = () => true) =>
    (await calls()).some((call) => call.command === command && test(call.args));
  try {
    await steps(page, called);
    if (errors.length) throw new Error(`page errors: ${errors.join(" | ")}`);
    console.log(`PASS  ${name}`);
  } catch (error) {
    failed += 1;
    console.log(`FAIL  ${name}: ${error.message.split("\n")[0]}`);
  }
  await page.close();
}

/** Opens a folded part of the minutes by its title. */
const unfold = async (page, title) => {
  await page.locator("summary", { hasText: title }).first().click();
  await page.waitForTimeout(200);
};

/** Opens a group of the settings from the side list. */
const part = async (page, name) => {
  await page.getByRole("tab", { name, exact: true }).click();
  await page.waitForTimeout(200);
};

const expect = (ok, what) => {
  if (!ok) throw new Error(what);
};

/** Walks the first run forward to step 3 (있으면 좋은 것). */
const toNiceToHave = async (page) => {
  for (let i = 0; i < 2; i += 1) {
    await page.getByRole("button", { name: "다음", exact: true }).click();
    await page.waitForTimeout(150);
  }
  await page
    .getByRole("heading", { name: "있으면 더 편한 것들" })
    .waitFor({ timeout: 3000 });
};

await flow(
  "first run: start saves the choice and watches the folder",
  "onboarding",
  async (page, called) => {
    expect(
      (await page.getByRole("button", { name: "서기 시작하기" }).count()) === 0,
      "the first step can already start",
    );
    await toNiceToHave(page);
    await page.getByRole("button", { name: "서기 시작하기" }).click();
    await page
      .getByRole("region", { name: "회의함" })
      .waitFor({ timeout: 5000 });
    expect(
      await called(
        "seogi_set_config",
        (a) =>
          a.config.set_up === true && a.config.summary.kind === "claude_cli",
      ),
      "the settings were not saved as set up with the chosen summary model",
    );
    expect(
      await called("seogi_watch_folder", (a) =>
        String(a.path).endsWith("녹음함"),
      ),
      "the recordings folder was not watched",
    );
  },
);

await flow(
  "first run: making the folder in the chosen Drive",
  "onboarding",
  async (page, called) => {
    await toNiceToHave(page);
    await page
      .getByRole("button", { name: "이 드라이브에 녹음함 만들기" })
      .click();
    await page.getByText("폴더를 만들었어요").waitFor({ timeout: 5000 });
    expect(
      await called("seogi_watch_folder", (a) =>
        String(a.path).includes("me@example.com"),
      ),
      "the folder was not made in the first account's Drive",
    );
  },
);

await flow(
  "home: a meeting goes to the trash from its line, and comes back",
  "home",
  async (page, called) => {
    const box = page.getByRole("region", { name: "회의함" });
    const rows = box.locator(".a-row");
    const before = await rows.count();
    await rows.first().click({ button: "right" });
    await page.getByRole("menuitem", { name: "휴지통으로 옮기기" }).click();
    const ask = page.getByRole("alertdialog", {
      name: "이 회의를 휴지통으로 옮길까요?",
    });
    expect(
      (await ask.getByText("Dock 오른쪽 끝").count()) === 1,
      "the question does not say where the trash is",
    );
    await ask.getByRole("button", { name: "휴지통으로" }).click();
    await page.waitForTimeout(300);
    expect(await called("seogi_delete_meeting"), "the meeting was not deleted");
    await page
      .getByRole("status")
      .getByRole("button", { name: "휴지통 열기" })
      .click();
    await page.waitForTimeout(200);
    expect(await called("seogi_open_trash"), "the trash was not opened");
    expect((await rows.count()) === before - 1, "the line stayed in the list");
    await page
      .getByRole("status")
      .getByRole("button", { name: "되돌리기" })
      .click();
    await page.waitForTimeout(300);
    expect(await called("seogi_undo_delete"), "undo was not asked for");
    expect((await rows.count()) === before, "the line did not come back");
  },
);

await flow(
  "minutes: deleting asks first, inside the window",
  "minutes",
  async (page, called) => {
    await page.getByRole("button", { name: "더 보기" }).first().click();
    await page.getByRole("menuitem", { name: "휴지통으로 옮기기" }).click();
    const ask = page.getByRole("alertdialog");
    await ask.getByRole("button", { name: "취소" }).click();
    expect((await ask.count()) === 0, "the question did not close");
    expect(
      !(await called("seogi_delete_meeting")),
      "it was deleted without a yes",
    );
  },
);

await flow("record, mark, stop", "home", async (page, called) => {
  await page
    .getByRole("button", { name: /^녹음 시작/ })
    .first()
    .click();
  await page
    .getByRole("button", { name: "끝내기", exact: true })
    .waitFor({ timeout: 5000 });
  expect(
    await called("plugin:macos-permissions|check_microphone_permission"),
    "the microphone was not asked for first",
  );
  expect(await called("seogi_start_recording"), "recording was not started");
  await page.getByRole("button", { name: "중요 표시" }).click();
  await page.waitForTimeout(300);
  expect(await called("seogi_mark"), "the mark was not sent");
  await page.getByRole("button", { name: "끝내기", exact: true }).click();
  await page.getByRole("region", { name: "회의함" }).waitFor({ timeout: 5000 });
  expect(await called("seogi_stop_recording"), "recording was not stopped");
});

await flow("recording an event from the band", "home", async (page, called) => {
  await page.getByRole("button", { name: /김서윤 \/ 초도 상담/ }).click();
  await page
    .getByRole("button", { name: "끝내기", exact: true })
    .waitFor({ timeout: 5000 });
  expect(
    await called(
      "seogi_start_recording",
      (a) => a.eventUid === "consult" && typeof a.eventStartMs === "number",
    ),
    "the recording was not tied to the event",
  );
});

await flow("ticking a task off", "minutes", async (page, called) => {
  await page.getByRole("checkbox").first().check();
  await page.waitForTimeout(300);
  expect(
    await called(
      "seogi_set_action_done",
      (a) => a.done === true && a.index === 0,
    ),
    "the task was not ticked off",
  );
});

await flow(
  "hearing five seconds of a doubtful line",
  "minutes",
  async (page, called) => {
    await unfold(page, "확인 필요");
    await page
      .getByRole("button", { name: /부터 5초 듣기/ })
      .first()
      .click();
    await page.waitForTimeout(800);
    expect(
      await called("seogi_audio_snippet", (a) => a.endMs - a.startMs === 6000),
      "the piece of the recording was not asked for",
    );
  },
);

await flow(
  "asking the recording a question",
  "minutes",
  async (page, called) => {
    await unfold(page, "이 녹음에 묻기");
    await page
      .getByRole("textbox", { name: "이 녹음에 묻기" })
      .fill("도면은 언제까지 보내나요?");
    await page.getByRole("button", { name: "묻기", exact: true }).click();
    await page
      .getByText("금요일까지 보내기로 했습니다")
      .waitFor({ timeout: 5000 });
    expect(
      await called("seogi_ask", (a) => String(a.question).includes("도면")),
      "the question was not asked",
    );
  },
);

await flow(
  "minutes: this meeting's site and note are kept, then used again",
  "minutes",
  async (page, called) => {
    await unfold(page, "이번 회의 정보");
    const place = page.getByRole("textbox", { name: "현장" });
    await place.fill("새봄마을 101동 1203호");
    await page.getByRole("textbox", { name: "메모" }).click();
    await page
      .getByRole("textbox", { name: "메모" })
      .fill("주방 위치를 정하는 회의");
    await page.getByRole("textbox", { name: "현장" }).click();
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_meeting_info",
        (a) => a.info.place === "새봄마을 101동 1203호",
      ),
      "the site was not saved",
    );
    expect(
      await called(
        "seogi_set_meeting_info",
        (a) => a.info.note === "주방 위치를 정하는 회의",
      ),
      "the note was not saved",
    );
    await page.getByRole("button", { name: "받아쓰기부터 다시" }).click();
    await page.waitForTimeout(300);
    expect(
      await called("seogi_retranscribe"),
      "transcribing again was not asked for",
    );
  },
);

await flow(
  "minutes: words are taken from documents by ticking only",
  "minutes",
  async (page, called) => {
    await unfold(page, "이번 회의 정보");
    await page.getByRole("button", { name: "자료에서 단어 뽑기" }).click();
    await page.getByRole("checkbox", { name: "포세린 타일" }).waitFor();
    expect(
      (await page.getByText("내 컴퓨터 모델로는 건너뜁니다").count()) > 0,
      "the skipped picture is not explained",
    );
    await page.getByRole("checkbox", { name: "걸레받이" }).uncheck();
    await page.getByRole("checkbox", { name: /용어집에도 넣기/ }).check();
    await page.getByRole("button", { name: "고른 단어 3개 넣기" }).click();
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_meeting_info",
        (a) =>
          Array.isArray(a.info.terms) &&
          a.info.terms.includes("새봄마루") &&
          !a.info.terms.includes("걸레받이"),
      ),
      "the ticked words were not added to the meeting",
    );
    expect(
      await called("seogi_add_to_vocabulary", (a) => a.terms.length === 3),
      "the vocabulary was not given the words",
    );
    await page.getByText("회의 단어").first().waitFor();
    expect(
      (await page.getByText("새봄마루", { exact: true }).count()) > 0,
      "the words are not shown as the meeting's",
    );
  },
);

await flow(
  "minutes: a speaker is named from here on, picked from the people",
  "minutes",
  async (page, called) => {
    await unfold(page, "받아쓴 글");
    await page
      .getByRole("button", { name: "박도현 소장 이름 바꾸기" })
      .first()
      .click();
    const namer = page.getByRole("group", { name: "말한 사람 이름 바꾸기" });
    await namer.getByRole("button", { name: "여기부터" }).click();
    await namer.getByRole("button", { name: "이서진", exact: true }).click();
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_name_speaker",
        (a) => a.name === "이서진" && a.scope === "from_here" && a.fromMs > 0,
      ),
      "the name was not given from that line on",
    );
    expect(
      (await namer.count()) === 0,
      "the namer stayed open after the change",
    );
  },
);

await flow(
  "minutes: a consultation shows its talking and feedback with times",
  "minutes",
  async (page, called) => {
    await unfold(page, "상담 피드백");
    await page.getByRole("region", { name: "상담 피드백" }).waitFor();
    expect(await called("seogi_talk"), "the talking was not measured");
    expect(
      (await page.getByText("기타 1명").count()) > 0,
      "a speaker who said almost nothing is not counted as one of the others",
    );
    expect(
      (await page.getByText("잘한 점", { exact: true }).count()) === 1,
      "the points are not shown",
    );
  },
);

await flow(
  "minutes: an unnamed speaker is named from what they said",
  "minutes",
  async (page, called) => {
    await page
      .getByRole("main")
      .getByRole("button", { name: "누구 목소리인지 고르기" })
      .click();
    const card = page.getByRole("region", {
      name: "말한 사람에게 이름 붙이기",
    });
    await card.getByText("이 말은 누가 했나요?").waitFor();
    expect(
      (await card.getByText(/^「/).count()) >= 1,
      "no line of theirs is shown",
    );
    // Pressing a name gives it at once.
    await card.getByRole("button", { name: "이서진", exact: true }).click();
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_name_speaker",
        (a) => a.name === "이서진" && a.scope === "all",
      ),
      "the speaker was not named",
    );
  },
);

await flow(
  "minutes: who was there is typed one name at a time, nothing suggested",
  "minutes",
  async (page, called) => {
    await page.getByRole("button", { name: /^참석/ }).first().click();
    expect(
      (await page.getByRole("button", { name: /^\+ / }).count()) === 0,
      "names are still suggested from elsewhere",
    );
    await page
      .getByText("이 컴퓨터에만 남고")
      .first()
      .waitFor({ timeout: 2000 });
    const typed = page.getByRole("textbox", { name: "이름 입력" }).first();
    await typed.fill("한별");
    await typed.press("Enter");
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_meeting_info",
        (a) => Array.isArray(a.info.people) && a.info.people.includes("한별"),
      ),
      "a typed name was not added",
    );
  },
);

await flow(
  "minutes: says it is the minutes, offers its files and renaming speakers",
  "minutes",
  async (page, called) => {
    await page.getByText("회의록", { exact: true }).first().waitFor();
    await page.getByRole("button", { name: "말한 사람 이름 바꾸기" }).click();
    await page
      .getByText("이름을 누르면 말한 사람 이름을 바꿀 수 있어요.")
      .waitFor({ timeout: 2000 });
    await page
      .getByRole("button", { name: /더 보기|…|more/i })
      .first()
      .click();
    await page.getByRole("menuitem", { name: "회의록 파일 보기" }).click();
    await page.waitForTimeout(200);
    expect(
      await called("seogi_reveal", (a) => a.what === "minutes"),
      "the minutes file was not shown",
    );
  },
);

await flow(
  "recording: a note for the minutes can be left while recording",
  "recording",
  async (page, called) => {
    await page.locator("summary", { hasText: "고객 이름, 현장, 메모" }).click();
    const note = page.getByRole("textbox", { name: "회의 메모" });
    await note.fill("마감재 샘플 확인");
    await page.getByRole("textbox", { name: "현장" }).click();
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_meeting_info",
        (a) => a.info.note === "마감재 샘플 확인",
      ),
      "the note was not saved",
    );
  },
);

await flow(
  "settings: the minutes model in use is named, and can be tried",
  "settings",
  async (page, called) => {
    await page.getByText("지금 이걸로 써요: Claude").waitFor({ timeout: 2000 });
    await page.getByRole("button", { name: "시험해 보기" }).click();
    await page.getByText("됨 · 2.1초").waitFor({ timeout: 2000 });
    expect(await called("seogi_try_summary"), "the model was not tried");
  },
);

await flow(
  "settings: choosing this computer",
  "settings",
  async (page, called) => {
    await page.getByRole("radio", { name: "내 컴퓨터" }).check();
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_config",
        (a) => a.config.summary.kind === "ollama",
      ),
      "the summary model was not changed",
    );
  },
);

await flow(
  "settings: the model is picked from a list, the recommended first",
  "settings",
  async (page, called) => {
    const model = page.getByRole("combobox", { name: "모델" });
    const first = await model.locator("option").first().textContent();
    expect(first?.includes("권장"), "the recommended model is not first");
    expect(
      (await page.getByRole("textbox", { name: /모델/ }).count()) === 0,
      "a model name can still be typed in",
    );
    await model.selectOption("claude-opus-5-5");
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_config",
        (a) =>
          a.config.summary.kind === "claude_cli" &&
          a.config.summary.model === "claude-opus-5-5",
      ),
      "the chosen model was not saved",
    );
  },
);

await flow(
  "settings: the Mac's calendars are asked for once, then ticked",
  "settings",
  async (page, called) => {
    await part(page, "보관과 일정");
    await page
      .getByRole("button", { name: "맥 캘린더 앱에서 가져오기" })
      .click();
    await page.getByRole("checkbox", { name: /업무/ }).check();
    await page
      .getByText("맥 캘린더 연결됨", { exact: true })
      .waitFor({ timeout: 5000 });
    expect(
      await called("seogi_mac_calendar_request"),
      "the system was not asked",
    );
    expect(
      await called("seogi_set_config", (a) =>
        a.config.mac_calendars.includes("work"),
      ),
      "the chosen calendar was not saved",
    );
    expect(
      await called("seogi_refresh_calendar"),
      "the calendar was not read after choosing it",
    );
  },
);

await flow(
  "settings: a pasted calendar address is checked, saved and read",
  "settings",
  async (page, called) => {
    await part(page, "보관과 일정");
    await unfold(page, "더 보기");
    const field = page.getByRole("textbox", {
      name: "구글 캘린더 비공개 주소",
    });
    await field.fill("webcal://calendar.example/private-2/basic.ics");
    await field.press("Enter");
    await page.getByText("캘린더 2 · 연결됨").waitFor({ timeout: 5000 });
    expect(await called("seogi_check_calendar"), "the address was not checked");
    expect(
      await called("seogi_set_config", (a) =>
        a.config.calendar_urls.includes(
          "https://calendar.example/private-2/basic.ics",
        ),
      ),
      "the address was not saved as https",
    );
    expect(
      await called("seogi_refresh_calendar"),
      "the calendar was not read after saving",
    );
    expect(
      (await page.getByText("다음 일정:").count()) > 0,
      "the next event is not shown",
    );
  },
);

await flow(
  "settings: an address that is not one says why",
  "settings",
  async (page) => {
    await part(page, "보관과 일정");
    await unfold(page, "더 보기");
    const field = page.getByRole("textbox", {
      name: "구글 캘린더 비공개 주소",
    });
    await field.fill("someone@example.com");
    await page.getByRole("button", { name: "연결" }).click();
    await page.getByRole("alert").waitFor({ timeout: 5000 });
    const text = await page.getByRole("alert").textContent();
    expect(text?.includes("비공개 주소(iCal 형식)"), "no reason was given");
  },
);

await flow(
  "settings: the side list moves by arrow keys and is remembered",
  "settings",
  async (page) => {
    const first = page.getByRole("tab", { name: "회의록 정리" });
    expect(
      (await first.getAttribute("aria-selected")) === "true",
      "the first group is not chosen at the start",
    );
    await first.focus();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    const dictation = page.getByRole("tab", { name: "말로 입력", exact: true });
    expect(
      (await dictation.getAttribute("aria-selected")) === "true",
      "two arrow presses did not reach dictation",
    );
    expect(
      await dictation.evaluate((el) => el === document.activeElement),
      "the focus did not follow the arrow keys",
    );
    await page
      .getByRole("switch", { name: "말로 입력", exact: true })
      .waitFor();
    await page.getByRole("button", { name: "회의함" }).click();
    await page.getByRole("button", { name: "설정" }).first().click();
    await page.waitForTimeout(300);
    expect(
      (await page
        .getByRole("tab", { name: "말로 입력", exact: true })
        .getAttribute("aria-selected")) === "true",
      "the group looked at last was not opened again",
    );
  },
);

await flow(
  "first run: a calendar pasted just before start is kept",
  "onboarding",
  async (page, called) => {
    await toNiceToHave(page);
    await page.getByRole("radio", { name: "주소로 넣기" }).click();
    await page
      .getByRole("textbox", { name: "구글 캘린더 비공개 주소" })
      .fill("https://calendar.example/private-3/basic.ics");
    await page.getByRole("button", { name: "서기 시작하기" }).click();
    await page
      .getByRole("region", { name: "회의함" })
      .waitFor({ timeout: 5000 });
    expect(
      await called("seogi_set_config", (a) =>
        a.config.calendar_urls.includes(
          "https://calendar.example/private-3/basic.ics",
        ),
      ),
      "the calendar address was lost",
    );
  },
);

await flow(
  "first run: deciding later goes on to the permissions and keeps the way",
  "onboarding",
  async (page, called) => {
    await page.getByRole("radio", { name: "내 컴퓨터" }).check();
    await page.getByRole("button", { name: "나중에 정하기" }).click();
    await page
      .getByRole("heading", { name: "먼저 이것만 허락해 주세요" })
      .waitFor({ timeout: 3000 });
    expect(
      (await page.getByText("회의를 녹음하거나 말로 입력할 때 써요.").count()) >
        0,
      "deciding later skipped the microphone",
    );
    expect(
      !(await called("seogi_set_config")),
      "deciding later saved before the last step",
    );
    await page.getByRole("button", { name: "다음", exact: true }).click();
    await page.getByRole("button", { name: "서기 시작하기" }).click();
    await page
      .getByRole("region", { name: "회의함" })
      .waitFor({ timeout: 5000 });
    expect(
      await called(
        "seogi_set_config",
        (a) =>
          a.config.set_up === true && a.config.summary.kind === "claude_cli",
      ),
      "the way of writing the minutes did not stay as it was",
    );
  },
);

await flow(
  "first run: going back a step keeps what was chosen",
  "onboarding",
  async (page) => {
    await page.getByRole("radio", { name: "내 컴퓨터" }).check();
    await toNiceToHave(page);
    await page.getByRole("radio", { name: "주소로 넣기" }).click();
    const field = page.getByRole("textbox", {
      name: "구글 캘린더 비공개 주소",
    });
    await field.fill("https://calendar.example/private-4/basic.ics");
    await page.getByRole("button", { name: "이전" }).click();
    await page.getByRole("button", { name: "이전" }).click();
    expect(
      await page.getByRole("radio", { name: "내 컴퓨터" }).isChecked(),
      "the way of writing the minutes was forgotten",
    );
    await toNiceToHave(page);
    expect(
      (await field.inputValue()) ===
        "https://calendar.example/private-4/basic.ics",
      "the calendar address was forgotten",
    );
  },
);

await flow(
  "first run: skipping step 3 keeps the choice and connects nothing",
  "onboarding",
  async (page, called) => {
    await toNiceToHave(page);
    await page.getByRole("radio", { name: "주소로 넣기" }).click();
    await page
      .getByRole("textbox", { name: "구글 캘린더 비공개 주소" })
      .fill("https://calendar.example/private-5/basic.ics");
    await page.getByRole("button", { name: "건너뛰고 시작하기" }).click();
    await page
      .getByRole("region", { name: "회의함" })
      .waitFor({ timeout: 5000 });
    expect(
      await called(
        "seogi_set_config",
        (a) =>
          a.config.set_up === true && a.config.summary.kind === "claude_cli",
      ),
      "the chosen way of writing the minutes was not saved",
    );
    expect(
      !(await called("seogi_set_config", (a) =>
        a.config.calendar_urls.includes(
          "https://calendar.example/private-5/basic.ics",
        ),
      )),
      "a skipped calendar address was saved",
    );
    expect(
      !(await called("seogi_watch_folder")),
      "a skipped folder was watched",
    );
  },
);

await flow(
  "first run: the Mac's Calendar is the first choice and asks the Mac",
  "onboarding",
  async (page, called) => {
    await toNiceToHave(page);
    expect(
      await page.getByRole("radio", { name: "맥 캘린더" }).isChecked(),
      "the Mac's Calendar is not the first choice",
    );
    expect(
      (await page
        .getByRole("textbox", { name: "구글 캘린더 비공개 주소" })
        .count()) === 0,
      "the address field shows without '주소로 넣기'",
    );
    await page
      .getByRole("table", { name: "일정과 폰 녹음 연결" })
      .getByRole("button", { name: "허락하기" })
      .click();
    await page.waitForTimeout(300);
    expect(
      await called("seogi_mac_calendar_request"),
      "the Mac was not asked for the calendar",
    );
  },
);

await flow(
  "first run on Windows: the calendar is an address",
  "onboarding&os=windows",
  async (page) => {
    await toNiceToHave(page);
    expect(
      (await page.getByRole("radio", { name: "맥 캘린더" }).count()) === 0,
      "Windows is offered the Mac's Calendar",
    );
    await page
      .getByRole("textbox", { name: "구글 캘린더 비공개 주소" })
      .waitFor({ timeout: 2000 });
  },
);

await flow(
  "home: a new version is offered, and waits while minutes are written",
  "home&update",
  async (page) => {
    await page.getByText("새 버전 0.1.1이 있어요").waitFor({ timeout: 5000 });
    const key = page.getByRole("button", { name: "설치" });
    expect(await key.isDisabled(), "it could be installed while busy");
    expect(
      (await page.getByText("끝나면 설치할 수 있어요").count()) > 0,
      "it does not say why it waits",
    );
  },
);

await flow(
  "banner: record the meeting that is starting",
  "banner-starting",
  async (page, called) => {
    await page.getByRole("button", { name: "녹음 시작" }).click();
    await page.waitForTimeout(300);
    expect(
      await called("seogi_banner_action", (a) => a.action === "record"),
      "the banner's record key did nothing",
    );
  },
);

await flow(
  "banner: open the minutes that are ready",
  "banner-ready",
  async (page, called) => {
    await page.getByRole("button", { name: "회의록 열기" }).click();
    await page.waitForTimeout(300);
    expect(
      await called("seogi_banner_action", (a) => a.action === "open"),
      "the banner's open key did nothing",
    );
  },
);

await flow(
  "dictation: turning it on saves it and shows the rest",
  "settings",
  async (page, called) => {
    await part(page, "말로 입력");
    expect(
      (await page.getByRole("combobox", { name: "말로 입력 모델" }).count()) ===
        0,
      "the dictation choices show while dictation is off",
    );
    await page.getByRole("switch", { name: "말로 입력", exact: true }).check();
    await page.waitForTimeout(300);
    expect(
      await called("seogi_set_dictation", (a) => a.dictation.enabled === true),
      "turning dictation on was not saved",
    );
    await page
      .getByRole("combobox", { name: "말로 입력 모델" })
      .waitFor({ timeout: 3000 });
  },
);

await flow(
  "dictation: where to allow typing is said on the screen, without asking for it",
  "settings-dictation",
  async (page, called) => {
    await page.getByText("손쉬운 사용에서").waitFor({ timeout: 3000 });
    await page.getByRole("button", { name: "시스템 설정 열기" }).click();
    await page.waitForTimeout(200);
    expect(
      await called("seogi_open_typing_permission"),
      "the button did not open System Settings",
    );
    expect(
      !(await called(
        "plugin:macos-permissions|request_accessibility_permission",
      )),
      "the screen asked the system for the permission itself",
    );
  },
);

await flow(
  "dictation: Option+Space by default, with a warning while Handy runs",
  "settings-dictation",
  async (page, called) => {
    const keys = page.getByRole("combobox", { name: "단축키" });
    const first = await keys.locator("option").first().textContent();
    expect(first?.startsWith("⌥Space"), "Option+Space is not the default");
    await page.getByText("다른 앱 Handy 도").waitFor({ timeout: 2000 });
    await keys.selectOption("ctrl+shift+space");
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_dictation",
        (a) => a.dictation.shortcut === "ctrl+shift+space",
      ),
      "the chosen shortcut was not saved",
    );
    expect(
      (await page.getByText("다른 앱 Handy 도").count()) === 0,
      "the Handy warning stays on a key Handy does not use",
    );
  },
);

await flow(
  "dictation: the tidying model is a list, fast ones recommended, slow ones warned",
  "settings-dictation",
  async (page, called) => {
    const tidy = page.getByRole("combobox", { name: "다듬는 모델" });
    const options = await tidy.locator("option").allTextContents();
    expect(
      options[0].includes("빠름") && options[0].includes("권장"),
      "the fast one is not first and recommended",
    );
    expect(
      options.some((o) => o.includes("느림") && o.includes("비추천")),
      "a slow one is not warned about",
    );
    await page.getByText("돈이 들지 않아요").waitFor({ timeout: 2000 });
    await tidy.selectOption({ index: 2 });
    await page.waitForTimeout(300);
    expect(
      await called(
        "seogi_set_dictation",
        (a) => a.dictation.tidy_with.kind === "claude_cli",
      ),
      "the chosen tidying model was not saved",
    );
    await page
      .getByText("다 쓰면 다듬지 않은 글이 그대로 들어가요")
      .waitFor({ timeout: 2000 });
  },
);

await flow(
  "dictation: the floating key is off until turned on",
  "settings-dictation",
  async (page, called) => {
    const key = page.getByRole("switch", { name: "떠 있는 마이크 단추" });
    expect(!(await key.isChecked()), "the floating key starts on");
    await key.check();
    await page.waitForTimeout(300);
    expect(
      await called("seogi_set_dictation", (a) => a.dictation.button === true),
      "the floating key was not saved",
    );
  },
);

await flow(
  "dictation: the floating key starts and stops a dictation, and offers its menu",
  "mic-idle",
  async (page, called) => {
    const key = page.getByRole("button", { name: "말로 입력" });
    await key.click();
    await page.waitForTimeout(200);
    expect(
      await called("seogi_dictation_toggle"),
      "a press did not start dictation",
    );
    await key.click({ button: "right" });
    await page.waitForTimeout(200);
    expect(
      await called(
        "seogi_mic_menu",
        (a) => a.hide === "단추 숨기기" && a.settings === "말로 입력 설정",
      ),
      "a right click did not offer hiding and the settings",
    );
  },
);

await flow(
  "dictation: words with nowhere to go are offered to copy",
  "indicator-nowhere",
  async (page, called) => {
    await page
      .locator(".scard")
      .getByText("넣을 곳이 없어요")
      .waitFor({ timeout: 3000 });
    await page.getByText("최근 말로 입력한 글에도 있어요").waitFor();
    await page.getByRole("button", { name: "복사" }).click();
    await page.getByText("복사됨").waitFor({ timeout: 2000 });
    expect(
      await called("seogi_copy_last_dictation"),
      "the words were not copied",
    );
  },
);

await browser.close();
process.exit(failed ? 1 : 0);
