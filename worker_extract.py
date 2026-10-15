import os
import json
import re
import subprocess
import shutil
import sys
import time

SCANS_BASE = r"C:\workspace\jamie-exam\data\scans"
DATA_DIR = r"C:\workspace\cli_as_driver\backend\tutor_data"
RUNS_BASE = os.path.join(DATA_DIR, "runs_extract")
OUTPUT_DIR = os.path.join(DATA_DIR, "extracted_pages")

os.makedirs(RUNS_BASE, exist_ok=True)
os.makedirs(OUTPUT_DIR, exist_ok=True)

PROMPT_TEMPLATE = """請仔細閱讀當前目錄的 ./page.jpg 圖片，萃取該頁面上所有的題目（是非題、選擇題、填空題、簡答題）。
請輸出一個合法 JSON 陣列到 ./out/extracted.json。
每題格式：
{{
  "stem": "題目文字（請完整辨識繁體中文）",
  "type": "true_false 或 single_choice 或 handwrite_blank",
  "options": ["選項1", "選項2", "選項3", "選項4"],
  "answer": "如果是選擇題請填1-based整數(如1,2,3,4)；是非題填O或X；填空題填答案文字",
  "explanation": "題目解析或說明",
  "keywords": ["關鍵詞1", "關鍵詞2"]
}}
注意事項：
1. 是非題 options 請給 ["O", "X"]；選擇題 options 請給出選項文字清單；填空題 options 為 []。
2. 請直接將結果以 UTF-8 JSON 陣列寫入 ./out/extracted.json，不要輸出其他檔案或說明文字。
"""

def process_page(subject, filename):
    m = re.search(r'(\d+)', filename)
    page_num = int(m.group(1)) if m else 0

    sub_key = "social" if "社" in subject else "nature"
    page_id = f"{sub_key}_{page_num:03d}"
    final_json = os.path.join(OUTPUT_DIR, f"{page_id}.json")

    if os.path.exists(final_json):
        try:
            data = json.load(open(final_json, encoding="utf-8-sig"))
            if isinstance(data, list):
                print(f"[{page_id}] Already processed ({len(data)} questions), skip.")
                return True
        except Exception:
            pass

    src_img = os.path.join(SCANS_BASE, subject, filename)
    run_dir = os.path.join(RUNS_BASE, page_id)
    os.makedirs(os.path.join(run_dir, "out"), exist_ok=True)

    dest_img = os.path.join(run_dir, "page.jpg")
    if not os.path.exists(dest_img):
        shutil.copy(src_img, dest_img)

    prompt_file = os.path.join(run_dir, "prompt.txt")
    with open(prompt_file, "w", encoding="utf-8") as f:
        f.write(PROMPT_TEMPLATE)

    out_file = os.path.join(run_dir, "out", "extracted.json")
    if os.path.exists(out_file):
        try:
            os.remove(out_file)
        except Exception:
            pass

    print(f"[{page_id}] Running agy vision extraction...")
    cmd = 'Get-Content prompt.txt -Raw | agy.exe --dangerously-skip-permissions --output-format stream-json'
    try:
        res = subprocess.run(
            ["powershell", "-Command", cmd],
            cwd=run_dir,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            timeout=180
        )
    except subprocess.TimeoutExpired:
        print(f"[{page_id}] Process timed out, checking if output was produced...")

    if os.path.exists(out_file):
        try:
            data = json.load(open(out_file, encoding="utf-8-sig"))
            if isinstance(data, list):
                shutil.copy(out_file, final_json)
                print(f"[{page_id}] Extracted {len(data)} questions successfully!")
                return True
        except Exception as e:
            print(f"[{page_id}] JSON error: {e}")

    print(f"[{page_id}] Extraction yielded no valid questions.")
    return False

def compile_all():
    files = sorted(os.listdir(OUTPUT_DIR))
    all_questions = []

    for f in files:
        if not f.endswith(".json") or f.startswith("questions"):
            continue
        sub_key, page_str = f.replace(".json", "").split("_")
        page_num = int(page_str)
        sub_name = "社會" if sub_key == "social" else "自然"

        try:
            page_data = json.load(open(os.path.join(OUTPUT_DIR, f), encoding="utf-8-sig"))
            for idx, q in enumerate(page_data):
                qid = f"{sub_key}_{page_num:03d}_{idx+1:02d}"
                qtype = q.get("type", "single_choice")
                opts = q.get("options", [])
                ans = q.get("answer")

                if qtype == "true_false":
                    opts = ["O", "X"]
                    ans_val = "O" if str(ans).strip().upper() in ["O", "TRUE", "1"] else "X"
                elif qtype == "single_choice":
                    try:
                        ans_val = int(ans)
                    except:
                        ans_val = 1
                else:
                    ans_val = str(ans)

                all_questions.append({
                    "id": qid,
                    "subject": sub_name,
                    "unit": f"{sub_name} 第 {page_num} 頁",
                    "section": f"第 {page_num} 頁測驗",
                    "type": qtype,
                    "stem": q.get("stem", ""),
                    "options": opts,
                    "answer": ans_val,
                    "explanation": q.get("explanation", ""),
                    "keywords": q.get("keywords", [sub_name]),
                    "char_count": None,
                    "image": None,
                    "need_handwriting": False,
                    "book_page": page_num
                })
        except Exception as e:
            print(f"Error compiling {f}: {e}")

    target = os.path.join(DATA_DIR, "questions_scanned_all.json")
    with open(target, "w", encoding="utf-8") as f:
        json.dump(all_questions, f, ensure_ascii=False, indent=2)

    print(f"Compiled total {len(all_questions)} questions into {target}")

if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "compile":
        compile_all()
    elif len(sys.argv) >= 4:
        # e.g. python worker.py 社會 2 10
        sub = sys.argv[1]
        start_p = int(sys.argv[2])
        end_p = int(sys.argv[3])
        sub_folder = os.path.join(SCANS_BASE, sub)
        files = sorted([f for f in os.listdir(sub_folder) if '解答' not in f and f.endswith('.jpg')])

        for f in files:
            m = re.search(r'(\d+)', f)
            if m:
                p = int(m.group(1))
                if start_p <= p <= end_p:
                    process_page(sub, f)
        compile_all()
    else:
        print("Usage:")
        print("  python worker.py <社會|自然> <start_page> <end_page>")
        print("  python worker.py compile")
