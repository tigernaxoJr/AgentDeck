# 題庫管線（bank-pipeline）

把評量掃描頁（`<JAMIE_EXAM_ROOT>/data/scans/<社會|自然>/*.jpg`）交給 `agy` 辨識成題目 JSON，
再用解答頁校正答案，最後產出 `tutor_data/questions_scanned_all.json`。
後端啟動時若題庫為空，會自動匯入 `tutor_data/questions*.json`（見 `backend/src/tutor/features/bank_import.rs`）。

> 僅支援 Windows：腳本透過 PowerShell 呼叫 `agy.exe --dangerously-skip-permissions`。
> 題庫素材有版權，不放進 repo（`backend/tutor_data/` 已在 `.gitignore`）。

## 設定

路徑集中在 [`config.py`](config.py)，可用環境變數覆寫：

| 變數 | 預設 | 說明 |
|------|------|------|
| `JAMIE_EXAM_ROOT` | `C:\workspace\jamie-exam` | 素材根目錄 |
| `TUTOR_DATA_DIR` | `<repo>/backend/tutor_data` | 產出目錄（與後端相同） |

## 執行順序

```powershell
cd tools/bank-pipeline

# 1. 逐頁萃取題目 → tutor_data/extracted_pages/<頁>.json，結束時自動 compile
python worker_extract.py 社會 2 10
python worker_extract.py 自然 2 10

# 2. 檢查並修正萃取結果（就地改寫 extracted_pages/）
python verify_answers.py

# 3. 辨識解答頁 → tutor_data/parsed_answers/pages/（可只跑單一科目）
python batch_answers.py 社會

# 4. 以官方解答比對、校正答案 → tutor_data/questions_scanned_all.json
python apply_official_answers.py
```

只想重新合併已萃取的頁面：`python worker_extract.py compile`。

## 早期原型（保留參考，不在主流程內）

- `batch_extract.py`：第一版萃取腳本，已由 `worker_extract.py` 取代。
- `merge_extracted.py`：把單次測試輸出（`scratch/test_extract`）轉成 `questions_extra.json`。
