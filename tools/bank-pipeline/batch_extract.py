import os
import json
import subprocess
import glob

from config import DATA_DIR, SCANS_DIR
OUTPUT_DIR = os.path.join(DATA_DIR, "extracted_bank")
os.makedirs(OUTPUT_DIR, exist_ok=True)

PROMPT_TEMPLATE = """請仔細閱讀當前目錄的 ./page.jpg 圖片，萃取該頁面上所有的題目（是非題、選擇題、填空題、簡答題）。
請輸出一個合法 JSON 陣列到 ./out/extracted.json。
每題格式：
{{
  "id": "{subject_prefix}_{page_num:03d}_{{idx}}",
  "subject": "{subject_name}",
  "unit": "{subject_name} 第 {page_num} 頁",
  "section": "評量測驗",
  "type": "true_false 或 single_choice 或 handwrite_blank",
  "stem": "題目文字（請完整辨識繁體中文）",
  "options": ["選項1", "選項2", "選項3", "選項4"],
  "answer": "若是選擇題請填1-based整數(如1,2,3,4)；是非題填O或X；填空題填答案文字",
  "explanation": "題目解析或說明",
  "keywords": ["關鍵詞1", "關鍵詞2"],
  "book_page": {page_num}
}}
注意事項：
1. 若該題包含插圖，type 可為 handwrite_blank 或 single_choice。
2. options 在是非題時為 ["O", "X"]，選擇題時為 4 個選項文字，填空題為空陣列 []。
3. 請直接將結果 JSON 寫入 ./out/extracted.json，不要有其他文字。
"""

def extract_page(image_path, subject_name, subject_prefix, page_num, run_dir):
    os.makedirs(os.path.join(run_dir, "out"), exist_ok=True)
    # Copy image
    target_img = os.path.join(run_dir, "page.jpg")
    if not os.path.exists(target_img):
        import shutil
        shutil.copy(image_path, target_img)

    prompt = PROMPT_TEMPLATE.format(
        subject_name=subject_name,
        subject_prefix=subject_prefix,
        page_num=page_num,
    )
    prompt_file = os.path.join(run_dir, "prompt.txt")
    with open(prompt_file, "w", encoding="utf-8") as f:
        f.write(prompt)

    out_file = os.path.join(run_dir, "out", "extracted.json")
    if os.path.exists(out_file) and os.path.getsize(out_file) > 10:
        print(f"[{subject_prefix} Page {page_num}] Already extracted, skipping.")
        return json.load(open(out_file, encoding="utf-8"))

    print(f"[{subject_prefix} Page {page_num}] Spawning AI extraction...")
    cmd = 'Get-Content prompt.txt -Raw | agy.exe --dangerously-skip-permissions --output-format stream-json'
    res = subprocess.run(
        ["powershell", "-Command", cmd],
        cwd=run_dir,
        capture_output=True,
        text=True,
        timeout=180
    )

    if os.path.exists(out_file):
        try:
            return json.load(open(out_file, encoding="utf-8"))
        except Exception as e:
            print(f"[{subject_prefix} Page {page_num}] JSON parse error:", e)
    return []

if __name__ == "__main__":
    print("Extractor pipeline ready.")
