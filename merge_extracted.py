import os
import json
import re

EXTRACTED_FILE = r"C:\workspace\cli_as_driver\scratch\test_extract\out\extracted.json"
TARGET_QUESTIONS = r"C:\workspace\cli_as_driver\backend\tutor_data\questions_extra.json"

def convert_extracted():
    if not os.path.exists(EXTRACTED_FILE):
        print("No extracted file found.")
        return

    data = json.load(open(EXTRACTED_FILE, encoding="utf-8"))
    converted = []

    for i, item in enumerate(data):
        qid = f"soc_02_{i+1:02d}"
        qtype = item.get("type", "single_choice")
        ans = item.get("answer")
        opts = item.get("options", [])

        if qtype == "true_false":
            if not opts:
                opts = ["O", "X"]
            ans_val = "O" if str(ans).strip().upper() in ["O", "TRUE", "1"] else "X"
        elif qtype == "single_choice":
            try:
                ans_val = int(ans)
            except:
                ans_val = 1
        else:
            ans_val = str(ans)

        converted.append({
            "id": qid,
            "subject": "社會",
            "unit": "第 2 回 家鄉的地圖與運用 (2)",
            "section": "評量測驗",
            "type": qtype,
            "stem": item.get("stem", ""),
            "options": opts,
            "answer": ans_val,
            "explanation": item.get("explanation", ""),
            "keywords": item.get("keywords", ["地圖", "社會"]),
            "char_count": None,
            "image": None,
            "need_handwriting": False
        })

    with open(TARGET_QUESTIONS, "w", encoding="utf-8") as f:
        json.dump(converted, f, ensure_ascii=False, indent=2)

    print(f"Successfully converted {len(converted)} questions to {TARGET_QUESTIONS}")

if __name__ == "__main__":
    convert_extracted()
