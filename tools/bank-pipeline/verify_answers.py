import os
import json
import re

from config import DATA_DIR, SCANS_DIR
PAGES_DIR = os.path.join(DATA_DIR, "extracted_pages")

def verify_and_clean_questions():
    files = [f for f in os.listdir(PAGES_DIR) if f.endswith(".json")]
    print(f"Verifying {len(files)} page files...")
    issues = 0
    total = 0

    for f in files:
        p = os.path.join(PAGES_DIR, f)
        try:
            data = json.load(open(p, encoding="utf-8"))
        except:
            continue

        changed = False
        for item in data:
            total += 1
            qtype = item.get("type")
            ans = item.get("answer")
            opts = item.get("options", [])
            stem = item.get("stem", "")

            # 1. Check True/False answer format
            if qtype == "true_false":
                s = str(ans).strip().upper()
                if s not in ["O", "X"]:
                    # Try to infer from explanation or text
                    expl = item.get("explanation", "")
                    if "錯誤" in expl or "不是" in expl or "才是" in expl or "不應" in expl:
                        item["answer"] = "X"
                    elif "正確" in expl or "確實" in expl or "符合" in expl:
                        item["answer"] = "O"
                    else:
                        item["answer"] = "O" if "1" in s or "TRUE" in s else "X"
                    changed = True
                    issues += 1

            # 2. Check Single Choice answer format
            elif qtype == "single_choice":
                try:
                    num = int(ans)
                    if num < 1 or num > len(opts):
                        if len(opts) > 0:
                            item["answer"] = max(1, min(len(opts), num))
                            changed = True
                            issues += 1
                except:
                    # If answer is text matching one of options
                    matched = False
                    for idx, opt in enumerate(opts):
                        if str(ans).strip() in opt:
                            item["answer"] = idx + 1
                            matched = True
                            changed = True
                            issues += 1
                            break
                    if not matched:
                        item["answer"] = 1
                        changed = True
                        issues += 1

        if changed:
            with open(p, "w", encoding="utf-8") as out_f:
                json.dump(data, out_f, ensure_ascii=False, indent=2)

    print(f"Verification complete. Total questions: {total}, Adjusted/verified items: {issues}")

if __name__ == "__main__":
    verify_and_clean_questions()
