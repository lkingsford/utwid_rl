#!/bin/sh
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "study_name": "my_ebr_study",
    "status": "open"
  }' \
  http://127.0.0.1:5000/set_status


