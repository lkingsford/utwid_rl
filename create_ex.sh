#!/bin/sh
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "study_name": "my_ebr_study",
    "direction": "min,min,min,min,min,min",
    "x86_manylinux_wheel_s3": "wheels/mon2y-0.1.0.dev24-cp313-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
    "module": "mon2y.ebr_opt",
    "function": "trial_worker",
    "iterations": 1000
  }' \
  http://127.0.0.1:5000/create_study


