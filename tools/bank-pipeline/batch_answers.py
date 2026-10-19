import os
import json
import re
import subprocess
import shutil
import sys
import time
from concurrent.futures import ThreadPoolExecutor

from config import DATA_DIR, SCANS_DIR
SCANS_BASE = SCANS_DIR
ANSWERS_DIR = os.path.join(DATA_DIR, "parsed_answers")
RUNS_BASE = os.path.join(ANSWERS_DIR, "runs")
OUTPUT_DIR = os.path.join(ANSWERS_DIR, "pages")

os.makedirs(RUNS_BASE, exist_ok=True)
os.makedirs(OUTPUT_DIR, exist_ok=True)

PROMPT_TEMPLATE = """請仔細閱讀當前目錄的 ./page.jpg 圖片（這是一張評量解答篇掃描頁）。
請辨識出該解答頁面中所有單元、回數、頁碼標示、各大題題號與標準答案、以及題目解析（如有）。
請輸出一個合法 JSON 物件到 ./out/answers.json。
格式範例：
{
  "book_title": "解答篇標題",
  "answer_page": 83,
  "sections": [
    {
      "round": "第 1 回",
      "textbook_page": 6,
      "exercises": [
        {
          "part": "一",
          "type": "是非題",
          "answers": {
            "1": "✕",
            "2": "◯"
          }
        },
        {
          "part": "二",
          "type": "選擇題",
          "answers": {
            "1": "2",
            "2": "4"
          }
        }
      ],
      "explanations": [
        {
          "question": "一、1",
          "note": "解析說明"
        }
      ]
    }
  ]
}
注意：
1. 請盡可能詳細完整辨識所有大題（是非題、選擇題、綜合題、連連看、填代號等）的所有題號與標準答案。
2. 請直接寫入 ./out/answers.json，務必保持合法 JSON 格式。
"""

def process_answer_sheet(subject, filename):
    m = re.search(r'(\d+)', filename)
    page_num = int(m.group(1)) if m else 0

    sub_key = "social" if "社" in subject else "nature"
    page_id = f"{sub_key}_ans_{page_num:03d}"
    final_json = os.path.join(OUTPUT_DIR, f"{page_id}.json")

    if os.path.exists(final_json) and os.path.getsize(final_json) > 50:
        print(f"[{page_id}] Already processed, skip.")
        return True

    src_img = os.path.join(SCANS_BASE, subject, filename)
    run_dir = os.path.join(RUNS_BASE, page_id)
    os.makedirs(os.path.join(run_dir, "out"), exist_ok=True)

    dest_img = os.path.join(run_dir, "page.jpg")
    if not os.path.exists(dest_img):
        shutil.copy(src_img, dest_img)

    prompt_file = os.path.join(run_dir, "prompt.txt")
    with open(prompt_file, "w", encoding="utf-8-sig") as f:
        f.write(PROMPT_TEMPLATE)

    out_file = os.path.join(run_dir, "out", "answers.json")
    if os.path.exists(out_file):
        try:
            os.remove(out_file)
        except Exception:
            pass

    print(f"[{page_id}] Running agy vision extraction on answer sheet...")
    cmd = 'Get-Content prompt.txt -Encoding UTF8 -Raw | agy.exe --dangerously-skip-permissions --output-format stream-json'
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
        print(f"[{page_id}] Timeout!")
        return False

    if os.path.exists(out_file):
        try:
            data = json.load(open(out_file, encoding="utf-8-sig"))
            if isinstance(data, dict):
                shutil.copy(out_file, final_json)
                print(f"[{page_id}] Answer sheet extracted successfully!")
                return True
        except Exception as e:
            print(f"[{page_id}] JSON error: {e}")

    print(f"[{page_id}] Extraction yielded no valid json.")
    return False

def parse_all(subject_filter=None, max_workers=3):
    tasks = []
    for sub in ["自然", "社會"]:
        if subject_filter and subject_filter not in sub:
            continue
        sub_dir = os.path.join(SCANS_BASE, sub)
        if not os.path.exists(sub_dir):
            continue
        for f in sorted(os.listdir(sub_dir)):
            if "解答" in f and f.endswith(".jpg"):
                tasks.append((sub, f))

    print(f"Total answer sheets to parse: {len(tasks)}")
    with ThreadPoolExecutor(max_workers=max_workers) as executor:
        futures = [executor.submit(process_answer_sheet, sub, f) for sub, f in tasks]
        for fut in futures:
            fut.result()

if __name__ == "__main__":
    sub_arg = sys.argv[1] if len(sys.argv) > 1 else None
    parse_all(sub_arg, max_workers=3)
