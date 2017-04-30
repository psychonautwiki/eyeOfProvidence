FROM kenansulayman/rust-nightly:latest

ADD . /my-source

RUN cd /my-source && cargo build -v
#--release

CMD ["/my-source/target/release/eye_of_providence"]
