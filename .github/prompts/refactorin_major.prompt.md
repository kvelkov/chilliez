---
mode: task
---
Comprehensive Prompt (Full Process)
Objective: To analyze an entire codebase, understand its structure and functionality, and then refactor it for improved efficiency, readability, and maintainability.

Prompt:

"I want you to act as an expert software engineer specializing in code refactoring and architecture. Your task is to perform a complete analysis and subsequent refactoring of my codebase.

Phase 1: Code Analysis and Functional Mapping

Initial Scan: Begin by scanning the following directory: [Specify Starting Directory].

Iterative Analysis: Go through each folder one by one. For each folder, list every file it contains.

Functional Summary: For each file, provide a concise summary of its primary purpose, the functions or classes it defines, and its key responsibilities.

Create a Functional Map: Compile this information into a comprehensive "Functional Map" in Markdown. The map should clearly show the folder hierarchy, with each file and its functional summary nested within its parent folder.

Confirmation: After you have mapped a folder, show me the result. I will confirm when you can proceed to the next folder until the entire codebase is mapped.

Phase 2: Refactoring

Identify Refactoring Opportunities: Once the Functional Map is complete, analyze the relationships between different functions and modules. Identify opportunities for refactoring, such as:

Redundant or duplicate functions that can be consolidated.

Poorly placed functions that should be moved to a more logical module.

Large, monolithic functions that can be broken down into smaller, single-responsibility functions.

Opportunities to improve code organization and create a more modular design.

Propose a Refactoring Plan: Based on your analysis, create a detailed refactoring plan. For each proposed change, specify:

The file and function(s) to be changed.

The reason for the refactoring (e.g., "Consolidate duplicate logic," "Improve modularity").

The proposed new location or structure for the code.

Execute Refactoring: Upon my approval of the plan, proceed with generating the refactored code. Provide the modified code in a format that I can easily integrate back into my project."

Part 1: Code Analysis Prompt
Objective: To analyze a specific part of the codebase and create a functional map.

Prompt:

"I need you to analyze a portion of my codebase. Your task is to create a 'Functional Map' for the directory I provide.

Target Directory: Start with the folder located at: [Specify Folder Path].

File & Function Listing: List all the files within this directory. For each file, analyze its content and identify all the functions, classes, and main components.

Purpose Summary: For each identified function or class, write a brief, clear description of its purpose and what it does.

Generate Map: Present this information as a Markdown-formatted list. Use nested bullets to represent the folder/file/function hierarchy.

Example Output:

- /src/utils/
    - `api.js`
        - **Purpose:** Handles all external API communications.
        - **Functions:**
            - `fetchData(url)`: Fetches data from a given URL.
            - `postData(url, data)`: Sends data to a given URL.
    - `formatters.js`
        - **Purpose:** Contains utility functions for formatting data.
        - **Functions:**
            - `formatDate(date)`: Formats a date object into a readable string.
            - `capitalize(string)`: Capitalizes the first letter of a string.

I will provide you with folders one by one. Wait for my confirmation before proceeding to a new directory."

Part 2: Refactoring Prompt
Objective: To analyze the previously created functional map, identify refactoring opportunities, and propose a plan.

Prompt:

"Attached is the 'Functional Map' of my codebase that you helped create.

[Paste the Functional Map Here]

Now, acting as a senior software architect, I need you to analyze this map and the described functionalities to identify opportunities for refactoring.

Your task is to:

Analyze Function Dependencies: Examine the map and determine how different functions and modules interact. Look for functions that are misplaced or logic that is duplicated across different files.

Identify Refactoring Candidates: Pinpoint specific areas for improvement. Focus on:

Consolidation: Are there duplicate functions doing the same thing in different places?

Cohesion: Are functions located in the most logical and cohesive module? For instance, should a utility function in a component file be moved to a dedicated utils module?

Decoupling: Can we reduce dependencies between modules?

Create a Refactoring Plan: Based on your analysis, propose a clear, actionable refactoring plan. Structure your plan as a list of specific changes. For each change, describe:

What: The function/logic to be moved, merged, or split.

From: Its current file/location.

To: Its proposed new file/location or new structure.

Why: The reasoning for the change (e.g., 'To centralize all API logic,' 'To remove code duplication,' 'To improve separation of concerns').

Present this plan in a clear, easy-to-read format."