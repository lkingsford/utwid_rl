#!/bin/bash

# Stop and terminate the instance
echo "Terminating instance: i-0d6b62c0263d774f3"
aws ec2 terminate-instances --instance-ids "i-0d6b62c0263d774f3"

# Wait for instance to be terminated
echo "Waiting for instance to terminate..."
aws ec2 wait instance-terminated --instance-ids "i-0d6b62c0263d774f3"

# Delete this script
echo "Deleting stop script..."
rm -- "$0"

