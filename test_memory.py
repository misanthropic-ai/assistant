#!/usr/bin/env python3
"""Test script for memory tool functionality"""

import subprocess
import json
import sys

def send_command(command):
    """Send a command to the assistant and get response"""
    result = subprocess.run(
        ['./target/release/assistant'],
        input=command + '\n',
        capture_output=True,
        text=True
    )
    return result.stdout

def test_memory_operations():
    """Test various memory operations"""
    
    print("Testing Memory Tool Operations")
    print("=" * 50)
    
    # Test 1: Store memory with auto-generated key
    print("\n1. Testing Store operation:")
    cmd = json.dumps({
        "action": "store",
        "content": "The capital of France is Paris. It's known as the City of Light.",
        "metadata": {"category": "geography", "importance": "high"}
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 2: Store with explicit key
    print("\n2. Testing StoreWithKey operation:")
    cmd = json.dumps({
        "action": "store_with_key",
        "key": "rust-ownership",
        "content": "Rust's ownership system ensures memory safety without a garbage collector. Each value has a single owner.",
        "metadata": {"category": "programming", "language": "rust"}
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 3: Store another memory for search testing
    print("\n3. Storing another memory:")
    cmd = json.dumps({
        "action": "store_with_key",
        "key": "python-gc",
        "content": "Python uses reference counting and a garbage collector to manage memory automatically.",
        "metadata": {"category": "programming", "language": "python"}
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 4: Exact search
    print("\n4. Testing Exact search:")
    cmd = json.dumps({
        "action": "search",
        "query": "rust-ownership",
        "mode": "exact",
        "limit": 5
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 5: Keyword search
    print("\n5. Testing Keyword search:")
    cmd = json.dumps({
        "action": "search",
        "query": "memory garbage collector",
        "mode": "keyword",
        "limit": 5
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 6: Semantic search
    print("\n6. Testing Semantic search:")
    cmd = json.dumps({
        "action": "search",
        "query": "How does memory management work?",
        "mode": "semantic",
        "limit": 5
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 7: Hybrid search (default)
    print("\n7. Testing Hybrid search:")
    cmd = json.dumps({
        "action": "search",
        "query": "capital city France",
        "limit": 5
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 8: List memories
    print("\n8. Testing List operation:")
    cmd = json.dumps({
        "action": "list"
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 9: Retrieve specific memory
    print("\n9. Testing Retrieve operation:")
    cmd = json.dumps({
        "action": "retrieve",
        "key": "rust-ownership"
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")
    
    # Test 10: Get stats
    print("\n10. Testing Stats operation:")
    cmd = json.dumps({
        "action": "stats"
    })
    print(f"Command: {cmd}")
    response = send_command(cmd)
    print(f"Response: {response}")

if __name__ == "__main__":
    test_memory_operations()