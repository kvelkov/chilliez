import os
import google.generativeai as genai
from dotenv import load_dotenv

print("--- Starting Debug ---")

# 1. Load the .env file
# The load_dotenv() function will search for a .env file in the current directory
# and load the variables from it into the environment.
print("Attempting to load .env file...")
load_dotenv()
print(".env file loaded (if it was found).")

# 2. Get the API key from the environment
api_key = os.getenv("GOOGLE_API_KEY")

# 3. CRITICAL: Print the key to see if it was loaded
# We will only print the first 4 and last 4 characters for security.
if api_key:
    print(f"API Key from .env: {api_key[:4]}...{api_key[-4:]}")
else:
    print("API Key from .env: None") # <-- This is the problem indicator

# 4. Configure the SDK
if not api_key:
    print("\nERROR: API Key is None. Halting script. Check your .env file and its location.")
else:
    print("API Key found. Configuring Gemini...")
    try:
        genai.configure(api_key=api_key)
        
        # 5. Try to use the model
        print("Attempting to create model 'gemini-1.5-pro-latest'...")
        model = genai.GenerativeModel('gemini-1.5-pro-latest') # Using a more current model name
        
        print("Generating content...")
        response = model.generate_content("Give me a one-sentence, positive message.")
        
        print("\n--- Success! ---")
        print(response.text)
        print("------------------\n")

    except Exception as e:
        print(f"\n--- An Error Occurred ---")
        print(e)
        print("-------------------------\n")

print("--- Debug Finished ---")