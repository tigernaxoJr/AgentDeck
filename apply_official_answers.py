import os
import json
import re

DATA_DIR = r"C:\workspace\cli_as_driver\backend\tutor_data"
ANSWERS_DIR = os.path.join(DATA_DIR, "parsed_answers", "pages")
EXTRACTED_DIR = os.path.join(DATA_DIR, "extracted_pages")
ALL_QUESTIONS_FILE = os.path.join(DATA_DIR, "questions_scanned_all.json")

def normalize_ans(val):
    if val is None:
        return ""
    val_str = str(val).strip()
    # 符號歸一化
    val_str = val_str.replace("◯", "O").replace("○", "O").replace("O", "O")
    val_str = val_str.replace("✕", "X").replace("×", "X").replace("X", "X")
    # 數字選項 ①②③④ -> 1,2,3,4
    circle_nums = {"①": "1", "②": "2", "③": "3", "④": "4"}
    for k, v in circle_nums.items():
        val_str = val_str.replace(k, v)
    return val_str

def build_answer_maps():
    nature_answers = {} # textbook_page -> list of exercises
    social_answers = {} # round or page -> list of exercises
    
    # 1. 處理自然科解答
    for f in sorted(os.listdir(ANSWERS_DIR)):
        if "nature" in f and f.endswith(".json"):
            data = json.load(open(os.path.join(ANSWERS_DIR, f), encoding="utf-8-sig"))
            for sec in data.get("sections", []):
                p = sec.get("textbook_page")
                if p:
                    if p not in nature_answers:
                        nature_answers[p] = []
                    nature_answers[p].append(sec)

    # 2. 處理社會科解答
    for f in sorted(os.listdir(ANSWERS_DIR)):
        if "social" in f and f.endswith(".json"):
            data = json.load(open(os.path.join(ANSWERS_DIR, f), encoding="utf-8-sig"))
            for sec in data.get("sections", []):
                # 社會科可能標記 round (第 1 回) 或 page (數字)
                round_str = str(sec.get("round") or "")
                m_round = re.search(r'(\d+)', round_str)
                round_num = int(m_round.group(1)) if m_round else None
                
                # 收集進入 key
                key = f"round_{round_num}" if round_num else "default"
                if key not in social_answers:
                    social_answers[key] = []
                social_answers[key].append(sec)
                
    return nature_answers, social_answers

def match_and_verify():
    nature_answers, social_answers = build_answer_maps()
    print(f"Loaded Nature answer maps for {len(nature_answers)} pages.")
    print(f"Loaded Social answer maps for {len(social_answers)} rounds.")

    files = sorted(os.listdir(EXTRACTED_DIR))
    total_questions = 0
    nature_matched = 0
    social_matched = 0

    all_verified_questions = []

    for f in files:
        if not f.endswith(".json") or f.startswith("questions"):
            continue
        sub_key, page_str = f.replace(".json", "").split("_")
        page_num = int(page_str)
        is_social = (sub_key == "social")
        subject = "社會" if is_social else "自然"

        page_questions = json.load(open(os.path.join(EXTRACTED_DIR, f), encoding="utf-8-sig"))
        if not isinstance(page_questions, list):
            continue

        # 自然科比對
        if not is_social and page_num in nature_answers:
            sections = nature_answers[page_num]
            # 展開所有答案
            ans_flat = []
            for sec in sections:
                for ex in sec.get("exercises", []):
                    extype = ex.get("type", "")
                    for k, ans_val in ex.get("answers", {}).items():
                        ans_flat.append((extype, str(k), normalize_ans(ans_val)))
            
            # 依題型分組答案
            tf_ans = [a for extype, k, a in ans_flat if extype and "是非" in str(extype)]
            sc_ans = [a for extype, k, a in ans_flat if extype and "選擇" in str(extype)]
            
            tf_idx = 0
            sc_idx = 0
            for q in page_questions:
                qtype = q.get("type")
                orig_ans = normalize_ans(q.get("answer"))
                if qtype == "true_false" and tf_idx < len(tf_ans):
                    official = tf_ans[tf_idx]
                    tf_idx += 1
                    if official:
                        q["answer"] = official
                        nature_matched += 1
                elif qtype == "single_choice" and sc_idx < len(sc_ans):
                    official = sc_ans[sc_idx]
                    sc_idx += 1
                    if official:
                        q["answer"] = official
                        nature_matched += 1

        # 社會科比對 (依每頁對應回數推算)
        elif is_social:
            # 評量通常每2頁一回，第 4,5 頁第一回，6,7 頁第二回...
            # 或是若檔案中有標題則比對
            round_guess = (page_num // 2) - 1
            key = f"round_{round_guess}"
            if key in social_answers:
                sections = social_answers[key]
                ans_flat = []
                for sec in sections:
                    for ex in sec.get("questions", []) or sec.get("exercises", []):
                        sec_name = ex.get("section_name", "") or ex.get("type", "")
                        for k, ans_val in ex.get("answers", {}).items():
                            ans_flat.append((sec_name, str(k), normalize_ans(ans_val)))
                
                tf_ans = [a for sec_name, k, a in ans_flat if sec_name and "是非" in str(sec_name)]
                sc_ans = [a for sec_name, k, a in ans_flat if sec_name and "選擇" in str(sec_name)]

                tf_idx = 0
                sc_idx = 0
                for q in page_questions:
                    qtype = q.get("type")
                    if qtype == "true_false" and tf_idx < len(tf_ans):
                        official = tf_ans[tf_idx]
                        tf_idx += 1
                        if official:
                            q["answer"] = official
                            social_matched += 1
                    elif qtype == "single_choice" and sc_idx < len(sc_ans):
                        official = sc_ans[sc_idx]
                        sc_idx += 1
                        if official:
                            q["answer"] = official
                            social_matched += 1

        for idx, q in enumerate(page_questions):
            total_questions += 1
            all_verified_questions.append({
                "id": f"{sub_key}_p{page_num:03d}_{idx+1:02d}",
                "subject": subject,
                "unit_id": f"unit_{(page_num // 10) + 1}",
                "unit_title": f"第{(page_num // 10) + 1}單元",
                "qtype": q.get("type", "single_choice"),
                "stem": q.get("stem", ""),
                "options": q.get("options", []),
                "answer": str(q.get("answer", "1")),
                "explanation": q.get("explanation", ""),
                "difficulty": 1,
                "book_page": page_num
            })

    # 寫入最終題庫檔
    with open(ALL_QUESTIONS_FILE, "w", encoding="utf-8") as out_f:
        json.dump(all_verified_questions, out_f, ensure_ascii=False, indent=2)

    print(f"==================================================")
    print(f"Total verified questions compiled: {len(all_verified_questions)}")
    print(f"Nature questions matched with official key: {nature_matched}")
    print(f"Social questions matched with official key: {social_matched}")
    print(f"Output saved to: {ALL_QUESTIONS_FILE}")
    print(f"==================================================")

if __name__ == "__main__":
    match_and_verify()
