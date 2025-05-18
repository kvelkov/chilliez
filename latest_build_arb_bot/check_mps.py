from transformers import AutoTokenizer, AutoModelForCausalLM

model_id = "nvidia/Llama-3_1-Nemotron-Ultra-253B-v1"

tokenizer = AutoTokenizer.from_pretrained(model_id, trust_remote_code=True)
model = AutoModelForCausalLM.from_pretrained(model_id, trust_remote_code=True)

# Move model to MPS
model.to("mps")

# Prepare input tensor and send it to MPS
prompt = "Who are you?"
inputs = tokenizer(prompt, return_tensors="pt").to("mps")

# Generate text
outputs = model.generate(**inputs, max_new_tokens=150)
print(tokenizer.decode(outputs[0], skip_special_tokens=True))
