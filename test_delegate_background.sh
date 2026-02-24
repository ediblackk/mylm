#!/bin/bash
# Test script for worker delegation with background jobs
echo "Starting worker at $(date)"

# Background job 1: Create timestamp file after 2 seconds
(echo "Background job 1 started"; sleep 2; date > timestamp_2s.txt; echo "Background job 1 completed") &

# Background job 2: Create timestamp file after 5 seconds
(echo "Background job 2 started"; sleep 5; date > timestamp_5s.txt; echo "Background job 2 completed") &

# Background job 3: Create timestamp file after 10 seconds
(echo "Background job 3 started"; sleep 10; date > timestamp_10s.txt; echo "Background job 3 completed") &

# Wait for all background jobs to complete
wait

echo "All background jobs completed at $(date)"
ls -la timestamp_*.txt 2>/dev/null || echo "No timestamp files found yet"
